//! Named Cloudflare Tunnel + DNS via cloudflare-rs. `cloudflared` runs the connector.
//! Tunnel credentials live under `var/` (gitignored). The API cannot return the secret later.

use crate::config::Config;
use crate::error::ApiError;
use base64::Engine;
use cloudflare::endpoints::account::{list_accounts::ListAccountsParams, Account, ListAccounts};
use cloudflare::endpoints::cfd_tunnel::create_tunnel::{
    CreateTunnel, Params as CreateTunnelParams,
};
use cloudflare::endpoints::cfd_tunnel::delete_tunnel::{
    DeleteTunnel, Params as DeleteTunnelParams,
};
use cloudflare::endpoints::cfd_tunnel::list_tunnels::{ListTunnels, Params as ListTunnelsParams};
use cloudflare::endpoints::cfd_tunnel::{ConfigurationSrc, Tunnel};
use cloudflare::endpoints::dns::dns::{
    CreateDnsRecord, CreateDnsRecordParams, DnsContent, DnsRecord, ListDnsRecords,
    ListDnsRecordsParams, UpdateDnsRecord, UpdateDnsRecordParams,
};
use cloudflare::endpoints::zones::zone::{ListZones, ListZonesParams, Zone};
use cloudflare::framework::{
    auth::Credentials,
    client::{async_api::Client as CfClient, ClientConfig},
    Environment,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

const PREFERRED_ZONE: &str = "rudeless.ai";
const DEFAULT_HOSTNAME: &str = "parley.rudeless.ai";
const TUNNEL_NAME: &str = "parley";
/// Must not match `/widget/uctp.js`. Unanchored `/uctp.*` steals that file
/// and Cloudflare returns 502 from the WebSocket origin.
const UCTP_INGRESS_PATH: &str = "^/uctp$";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelCredentialsFile {
    #[serde(rename = "AccountTag")]
    pub account_tag: String,
    #[serde(rename = "TunnelID")]
    pub tunnel_id: String,
    #[serde(rename = "TunnelName")]
    pub tunnel_name: String,
    #[serde(rename = "TunnelSecret")]
    pub tunnel_secret: String,
}

#[derive(Debug, Clone)]
pub struct TunnelBindings {
    pub public_base: String,
    pub uctp_ws_url: String,
    pub hostname: String,
}

pub async fn ensure_and_run(config: &Config) -> Result<TunnelBindings, ApiError> {
    crate::tls::ensure_rustls_ring();
    let hostname = if config.public_hostname.is_empty() {
        DEFAULT_HOSTNAME.to_string()
    } else {
        config.public_hostname.clone()
    };
    // Nested uctp.parley.rudeless.ai is not on Cloudflare Universal SSL
    // (`*.rudeless.ai` covers one label). UCTP shares the HTTP hostname path.
    let state_dir = config.state_dir();
    std::fs::create_dir_all(&state_dir).map_err(|e| ApiError::internal(format!("var dir: {e}")))?;
    let creds_path = state_dir.join(format!("{TUNNEL_NAME}-tunnel.json"));

    let client = CfClient::new(
        Credentials::UserAuthToken {
            token: config.cloudflare_key.clone(),
        },
        ClientConfig::default(),
        Environment::Production,
    )
    .map_err(|e| ApiError::internal(format!("cloudflare client: {e}")))?;

    let account_id = resolve_account_id(&client, config).await?;

    let zones: Vec<Zone> = client
        .request(&ListZones {
            params: ListZonesParams {
                name: Some(PREFERRED_ZONE.to_string()),
                ..Default::default()
            },
        })
        .await
        .map_err(|e| ApiError::internal(format!("cloudflare zones: {e}")))?
        .result;
    let zone = pick_zone(&zones, &hostname)
        .ok_or_else(|| ApiError::internal(format!("cloudflare: no zone matching {hostname}")))?;

    let existing: Vec<Tunnel> = client
        .request(&ListTunnels {
            account_identifier: &account_id,
            params: ListTunnelsParams {
                name: Some(TUNNEL_NAME.to_string()),
                is_deleted: Some(false),
                ..Default::default()
            },
        })
        .await
        .map_err(|e| ApiError::internal(format!("cloudflare tunnels: {e}")))?
        .result;

    let tunnel_id = if let Some(t) = existing.into_iter().next() {
        let id = t.id.to_string();
        match load_secret(&creds_path, &id) {
            Ok(_) => id,
            Err(e) => {
                tracing::warn!(
                    tunnel_id = %id,
                    error = %e.detail,
                    "named tunnel credentials missing locally; recreating once"
                );
                client
                    .request(&DeleteTunnel {
                        account_identifier: &account_id,
                        tunnel_id: &id,
                        params: DeleteTunnelParams { cascade: true },
                    })
                    .await
                    .map_err(|e| ApiError::internal(format!("cloudflare delete tunnel: {e}")))?;
                create_named_tunnel(&client, &account_id, &creds_path).await?
            }
        }
    } else {
        create_named_tunnel(&client, &account_id, &creds_path).await?
    };

    let cname_target = format!("{tunnel_id}.cfargotunnel.com");
    ensure_cname(&client, &zone.id, &hostname, &cname_target).await?;

    write_cloudflared_config(&state_dir, &creds_path, &tunnel_id, &hostname, config)?;
    spawn_cloudflared(&state_dir, &tunnel_id)?;

    Ok(TunnelBindings {
        public_base: format!("https://{hostname}"),
        uctp_ws_url: format!("wss://{hostname}/uctp"),
        hostname,
    })
}

async fn resolve_account_id(client: &CfClient, config: &Config) -> Result<String, ApiError> {
    if !config.cloudflare_account_id.is_empty() {
        return Ok(config.cloudflare_account_id.clone());
    }
    let accounts: Vec<Account> = client
        .request(&ListAccounts {
            params: Some(ListAccountsParams::default()),
        })
        .await
        .map_err(|e| ApiError::internal(format!("cloudflare accounts: {e}")))?
        .result;
    accounts
        .into_iter()
        .next()
        .map(|a| a.id)
        .ok_or_else(|| ApiError::internal("cloudflare: no accounts (set CLOUDFLARE_ACCOUNT_ID)"))
}

async fn create_named_tunnel(
    client: &CfClient,
    account_id: &str,
    creds_path: &Path,
) -> Result<String, ApiError> {
    let mut secret = vec![0u8; 32];
    rand::thread_rng().fill_bytes(&mut secret);
    let config_src = ConfigurationSrc::Local;
    let created = client
        .request(&CreateTunnel {
            account_identifier: account_id,
            params: CreateTunnelParams {
                name: TUNNEL_NAME,
                tunnel_secret: &secret,
                config_src: &config_src,
                metadata: None,
            },
        })
        .await
        .map_err(|e| ApiError::internal(format!("cloudflare create tunnel: {e}")))?
        .result;
    let secret_b64 = base64::engine::general_purpose::STANDARD.encode(&secret);
    let id = created.id.to_string();
    persist_credentials(
        creds_path,
        &TunnelCredentialsFile {
            account_tag: account_id.to_string(),
            tunnel_id: id.clone(),
            tunnel_name: TUNNEL_NAME.to_string(),
            tunnel_secret: secret_b64,
        },
    )?;
    Ok(id)
}

fn pick_zone<'a>(zones: &'a [Zone], hostname: &str) -> Option<&'a Zone> {
    zones
        .iter()
        .find(|z| hostname == z.name || hostname.ends_with(&format!(".{}", z.name)))
        .or_else(|| zones.iter().find(|z| z.name == PREFERRED_ZONE))
        .or_else(|| zones.first())
}

fn load_secret(path: &PathBuf, tunnel_id: &str) -> Result<String, ApiError> {
    let raw = std::fs::read_to_string(path).map_err(|_| {
        ApiError::internal(format!(
            "cloudflare tunnel {tunnel_id} exists but {} is missing",
            path.display()
        ))
    })?;
    let file: TunnelCredentialsFile = serde_json::from_str(&raw)
        .map_err(|e| ApiError::internal(format!("tunnel credentials: {e}")))?;
    if file.tunnel_id != tunnel_id {
        return Err(ApiError::internal(
            "saved tunnel credentials do not match the named tunnel",
        ));
    }
    Ok(file.tunnel_secret)
}

fn persist_credentials(path: &Path, file: &TunnelCredentialsFile) -> Result<(), ApiError> {
    let json = serde_json::to_string_pretty(file)
        .map_err(|e| ApiError::internal(format!("encode tunnel credentials: {e}")))?;
    std::fs::write(path, json)
        .map_err(|e| ApiError::internal(format!("write tunnel credentials: {e}")))
}

async fn ensure_cname(
    client: &CfClient,
    zone_id: &str,
    hostname: &str,
    target: &str,
) -> Result<(), ApiError> {
    let existing: Vec<DnsRecord> = client
        .request(&ListDnsRecords {
            zone_identifier: zone_id,
            params: ListDnsRecordsParams {
                name: Some(hostname.to_string()),
                per_page: Some(20),
                ..Default::default()
            },
        })
        .await
        .map_err(|e| ApiError::internal(format!("cloudflare dns list: {e}")))?
        .result;

    let wanted = DnsContent::CNAME {
        content: target.to_string(),
    };
    if let Some(record) = existing.iter().find(|r| r.name == hostname) {
        let same = matches!(&record.content, DnsContent::CNAME { content } if content == target);
        if same {
            return Ok(());
        }
        client
            .request(&UpdateDnsRecord {
                zone_identifier: zone_id,
                identifier: &record.id,
                params: UpdateDnsRecordParams {
                    ttl: Some(1),
                    proxied: Some(true),
                    name: hostname,
                    content: wanted,
                },
            })
            .await
            .map_err(|e| ApiError::internal(format!("cloudflare dns update {hostname}: {e}")))?;
        return Ok(());
    }

    client
        .request(&CreateDnsRecord {
            zone_identifier: zone_id,
            params: CreateDnsRecordParams {
                ttl: Some(1),
                priority: None,
                proxied: Some(true),
                name: hostname,
                content: wanted,
            },
        })
        .await
        .map_err(|e| ApiError::internal(format!("cloudflare dns create {hostname}: {e}")))?;
    Ok(())
}

fn write_cloudflared_config(
    state_dir: &Path,
    creds_path: &Path,
    tunnel_id: &str,
    http_host: &str,
    config: &Config,
) -> Result<(), ApiError> {
    let http = local_origin(&config.bind_http, "http");
    let uctp = local_origin(&config.bind_uctp_ws, "http");
    let creds = abs_path(creds_path);
    let yml = format!(
        "tunnel: {tunnel_id}\ncredentials-file: {}\ningress:\n  - hostname: {http_host}\n    path: {UCTP_INGRESS_PATH}\n    service: {uctp}\n  - hostname: {http_host}\n    service: {http}\n  - service: http_status:404\n",
        creds.display()
    );
    std::fs::write(state_dir.join("cloudflared.yml"), yml)
        .map_err(|e| ApiError::internal(format!("write cloudflared.yml: {e}")))?;
    Ok(())
}

fn local_origin(bind: &str, scheme: &str) -> String {
    if bind.starts_with("http://") || bind.starts_with("https://") {
        bind.to_string()
    } else {
        format!("{scheme}://{bind}")
    }
}

fn abs_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

fn spawn_cloudflared(state_dir: &Path, tunnel_id: &str) -> Result<(), ApiError> {
    let cfg = abs_path(&state_dir.join("cloudflared.yml"));
    let log = std::fs::File::create(state_dir.join("cloudflared.log"))
        .map_err(|e| ApiError::internal(format!("cloudflared log: {e}")))?;
    let err = log
        .try_clone()
        .map_err(|e| ApiError::internal(format!("cloudflared log: {e}")))?;
    let mut child = Command::new("cloudflared")
        .arg("tunnel")
        .arg("--config")
        .arg(&cfg)
        .arg("--no-autoupdate")
        .arg("run")
        .arg(tunnel_id)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(err))
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| ApiError::internal(format!("spawn cloudflared: {e}")))?;
    tokio::spawn(async move {
        match child.wait().await {
            Ok(status) => tracing::warn!(?status, "cloudflared exited"),
            Err(e) => tracing::warn!(error = %e, "cloudflared wait failed"),
        }
    });
    Ok(())
}

pub fn widget_public_json(bindings: Option<&TunnelBindings>, config: &Config) -> serde_json::Value {
    let http = bindings
        .map(|b| b.public_base.clone())
        .unwrap_or_else(|| config.public_http_base());
    let uctp = bindings
        .map(|b| b.uctp_ws_url.clone())
        .unwrap_or_else(|| config.public_uctp_ws());
    json!({
        "uctp_ws_url": uctp,
        "http_base": http,
        "tenant_id": config.tenant_id,
        "hostname": bindings
            .map(|b| b.hostname.clone())
            .filter(|h| !h.is_empty())
            .unwrap_or_else(|| config.public_hostname.clone()),
        "widget_path": "/widget/",
        "desk_path": "/desk/",
    })
}

#[cfg(test)]
mod tests {
    use super::UCTP_INGRESS_PATH;

    #[test]
    fn uctp_ingress_is_exact_websocket_path() {
        assert_eq!(UCTP_INGRESS_PATH, "^/uctp$");
        assert!(
            !UCTP_INGRESS_PATH.contains("uctp.*"),
            "/widget/uctp.js must be served by HTTP, not the UCTP websocket origin"
        );
    }
}
