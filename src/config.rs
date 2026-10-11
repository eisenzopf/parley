use crate::error::ApiError;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;

#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_bind_http")]
    pub bind_http: String,
    #[serde(default = "default_bind_uctp")]
    pub bind_uctp_ws: String,
    #[serde(default = "default_bind_sip")]
    pub bind_sip: String,
    /// Optional dedicated Orchestrator SIP adapter for conference outbound calls.
    #[serde(default)]
    pub conference_sip_bind: Option<String>,
    #[serde(default)]
    pub conference_sip_from: String,
    #[serde(default)]
    pub conference_network: crate::conference_network::ConferenceNetwork,
    #[serde(default = "default_sqlite")]
    pub sqlite_path: String,
    #[serde(default = "default_blob")]
    pub blob_dir: String,
    #[serde(default = "default_tenant")]
    pub tenant_id: String,
    #[serde(default)]
    pub api_secret: String,
    #[serde(default)]
    pub operator_bootstrap_token: String,
    #[serde(default)]
    pub vapi_api_key: String,
    #[serde(default)]
    pub vapi_assistant_id: String,
    #[serde(default = "default_vapi_public")]
    pub vapi_public_base: String,
    #[serde(default)]
    pub sip_advertise: String,
    #[serde(default = "default_reopen")]
    pub reopen_window_secs: i64,
    #[serde(default = "default_max_voice")]
    pub max_voice_sessions_per_tenant: u32,
    #[serde(default)]
    pub ai_stub: bool,
    #[serde(default = "default_vapi_chat")]
    pub vapi_chat_mode: String,
    #[serde(default = "default_consent")]
    pub recording_consent: String,
    #[serde(default)]
    pub hours: WeeklyHours,
    #[serde(default)]
    pub telnyx_api_key: String,
    #[serde(default)]
    pub telnyx_public_key: String,
    #[serde(default)]
    pub telnyx_from: String,
    #[serde(default)]
    pub telnyx_messaging_profile_id: String,
    /// Private, operator-reviewed web enrollment records for live SMS.
    #[serde(default)]
    pub sms_enrollment_path: String,
    #[serde(default)]
    pub sms_campaign_id: String,
    #[serde(default)]
    pub cloudflare_key: String,
    #[serde(default)]
    pub cloudflare_account_id: String,
    #[serde(default)]
    pub public_hostname: String,
    #[serde(default)]
    pub uctp_public_ws: String,
    /// When true and `cloudflare_key` is set, create/reuse a named tunnel.
    /// Tests leave this false (`Config::default()`). Playwright sets `PARLEY_TUNNEL=0`.
    #[serde(default)]
    pub tunnel: bool,
    /// When false, never create/update Vapi or Telnyx resources (Playwright / CI).
    #[serde(default = "default_provision")]
    pub provision: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind_http: default_bind_http(),
            bind_uctp_ws: default_bind_uctp(),
            bind_sip: default_bind_sip(),
            conference_sip_bind: None,
            conference_sip_from: String::new(),
            conference_network: Default::default(),
            sqlite_path: default_sqlite(),
            blob_dir: default_blob(),
            tenant_id: default_tenant(),
            api_secret: String::new(),
            operator_bootstrap_token: String::new(),
            vapi_api_key: String::new(),
            vapi_assistant_id: String::new(),
            vapi_public_base: default_vapi_public(),
            sip_advertise: String::new(),
            reopen_window_secs: default_reopen(),
            max_voice_sessions_per_tenant: default_max_voice(),
            ai_stub: false,
            vapi_chat_mode: default_vapi_chat(),
            recording_consent: default_consent(),
            hours: WeeklyHours::default(),
            telnyx_api_key: String::new(),
            telnyx_public_key: String::new(),
            telnyx_from: String::new(),
            telnyx_messaging_profile_id: String::new(),
            sms_enrollment_path: String::new(),
            sms_campaign_id: String::new(),
            cloudflare_key: String::new(),
            cloudflare_account_id: String::new(),
            public_hostname: String::new(),
            uctp_public_ws: String::new(),
            tunnel: false,
            provision: default_provision(),
        }
    }
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("bind_http", &self.bind_http)
            .field("bind_uctp_ws", &self.bind_uctp_ws)
            .field("bind_sip", &self.bind_sip)
            .field("sqlite_path", &self.sqlite_path)
            .field("blob_dir", &self.blob_dir)
            .field("tenant_id", &self.tenant_id)
            .field("api_secret", &redact(&self.api_secret))
            .field(
                "operator_bootstrap_token",
                &redact(&self.operator_bootstrap_token),
            )
            .field("vapi_api_key", &redact(&self.vapi_api_key))
            .field("vapi_assistant_id", &self.vapi_assistant_id)
            .field("vapi_public_base", &self.vapi_public_base)
            .field("sip_advertise", &self.sip_advertise)
            .field("reopen_window_secs", &self.reopen_window_secs)
            .field(
                "max_voice_sessions_per_tenant",
                &self.max_voice_sessions_per_tenant,
            )
            .field("ai_stub", &self.ai_stub)
            .field("vapi_chat_mode", &self.vapi_chat_mode)
            .field("recording_consent", &self.recording_consent)
            .field("hours", &self.hours)
            .field("telnyx_api_key", &redact(&self.telnyx_api_key))
            .field("telnyx_public_key", &redact(&self.telnyx_public_key))
            .field("telnyx_from", &self.telnyx_from)
            .field(
                "telnyx_messaging_profile_id",
                &self.telnyx_messaging_profile_id,
            )
            .field("cloudflare_key", &redact(&self.cloudflare_key))
            .field("cloudflare_account_id", &self.cloudflare_account_id)
            .field("public_hostname", &self.public_hostname)
            .field("uctp_public_ws", &self.uctp_public_ws)
            .field("tunnel", &self.tunnel)
            .field("provision", &self.provision)
            .finish()
    }
}

fn redact(value: &str) -> &'static str {
    if value.is_empty() {
        ""
    } else {
        "<redacted>"
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WeeklyHours {
    #[serde(default)]
    pub timezone: String,
    #[serde(default)]
    pub windows: Vec<HoursWindow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoursWindow {
    pub days: Vec<u8>,
    pub start: String,
    pub end: String,
}

impl Config {
    pub fn load() -> Result<Self, ApiError> {
        Self::load_from_path(Path::new("config/default.toml"))
    }

    pub fn load_from_path(path: &Path) -> Result<Self, ApiError> {
        let _ = dotenvy::dotenv();
        let mut cfg = if path.exists() {
            let raw = std::fs::read_to_string(path)
                .map_err(|e| ApiError::internal(format!("read config: {e}")))?;
            toml::from_str(&raw).map_err(|e| ApiError::internal(format!("parse config: {e}")))?
        } else {
            Config::default()
        };
        overlay_env(&mut cfg);
        cfg.conference_network.overlay_env()?;
        Ok(cfg)
    }

    pub fn sip_advertise_uri(&self) -> String {
        if self.sip_advertise.is_empty() {
            format!("sip:{}", self.bind_sip)
        } else {
            self.sip_advertise.clone()
        }
    }

    pub fn vapi_configured(&self) -> bool {
        !self.vapi_api_key.is_empty()
    }

    pub fn telnyx_configured(&self) -> bool {
        !self.telnyx_api_key.is_empty()
    }

    pub fn tunnel_enabled(&self) -> bool {
        self.tunnel && !self.cloudflare_key.is_empty()
    }

    pub fn state_dir(&self) -> std::path::PathBuf {
        std::path::Path::new(&self.blob_dir)
            .parent()
            .map(std::path::Path::to_path_buf)
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| std::path::PathBuf::from("var"))
    }

    pub fn public_http_base(&self) -> String {
        if !self.vapi_public_base.is_empty() {
            self.vapi_public_base.clone()
        } else if !self.public_hostname.is_empty() {
            format!("https://{}", self.public_hostname)
        } else {
            format!("http://{}", self.bind_http)
        }
    }

    pub fn public_uctp_ws(&self) -> String {
        if !self.uctp_public_ws.is_empty() {
            self.uctp_public_ws.clone()
        } else {
            format!("ws://{}", self.bind_uctp_ws)
        }
    }

    pub fn hash_api_secret(&self) -> String {
        hash_secret(&self.api_secret)
    }
}

pub fn hash_secret(secret: &str) -> String {
    blake3::hash(secret.as_bytes()).to_hex().to_string()
}

pub fn keyed_mac(secret: &str, data: &[u8]) -> String {
    let key = blake3::hash(secret.as_bytes());
    let mut hasher = blake3::Hasher::new_keyed(key.as_bytes());
    hasher.update(data);
    hasher.finalize().to_hex().to_string()
}

fn overlay_env(cfg: &mut Config) {
    if let Ok(v) = std::env::var("CONFERENCE_SIP_BIND") {
        cfg.conference_sip_bind = Some(v);
    }
    if let Ok(v) = std::env::var("CONFERENCE_SIP_FROM") {
        cfg.conference_sip_from = v;
    }
    if let Some(v) = first_env(&["PARLEY_BIND_HTTP"]) {
        cfg.bind_http = v;
    }
    if let Some(v) = first_env(&["PARLEY_BIND_UCTP_WS"]) {
        cfg.bind_uctp_ws = v;
    }
    if let Some(v) = first_env(&["PARLEY_BIND_SIP"]) {
        cfg.bind_sip = v;
    }
    if let Some(v) = first_env(&["PARLEY_SQLITE", "PARLEY_SQLITE_PATH"]) {
        cfg.sqlite_path = v;
    }
    if let Some(v) = first_env(&["PARLEY_BLOB_DIR"]) {
        cfg.blob_dir = v;
    }
    if let Some(v) = first_env(&["PARLEY_TENANT"]) {
        cfg.tenant_id = v;
    }
    if let Some(v) = first_env(&["PARLEY_API_SECRET"]) {
        cfg.api_secret = v;
    }
    if let Some(v) = first_env(&["PARLEY_OPERATOR_BOOTSTRAP"]) {
        cfg.operator_bootstrap_token = v;
    }
    if let Some(v) = first_env(&["PARLEY_VAPI_KEY", "VAPI_PRIVATE_KEY", "VAPI_API_KEY"]) {
        cfg.vapi_api_key = v;
    }
    if let Some(v) = first_env(&["PARLEY_VAPI_ASSISTANT", "VAPI_ASSISTANT_ID"]) {
        cfg.vapi_assistant_id = v;
    }
    if let Some(v) = first_env(&["PARLEY_PUBLIC_BASE", "VAPI_PUBLIC_BASE"]) {
        cfg.vapi_public_base = v;
    }
    if let Some(v) = first_env(&["PARLEY_SIP_ADVERTISE"]) {
        cfg.sip_advertise = v;
    }
    if let Some(v) = first_env(&["PARLEY_VAPI_CHAT"]) {
        cfg.vapi_chat_mode = v;
    }
    if let Some(v) = first_env(&["PARLEY_TELNYX_KEY", "TELNYX_TEST_API_KEY", "TELNYX_API_KEY"]) {
        cfg.telnyx_api_key = v;
    }
    if let Some(v) = first_env(&["PARLEY_TELNYX_PUBLIC_KEY", "TELNYX_PUBLIC_KEY"]) {
        cfg.telnyx_public_key = v;
    }
    if let Some(v) = first_env(&["PARLEY_TELNYX_FROM", "TELNYX_FROM_NUMBER"]) {
        cfg.telnyx_from = v;
    }
    if let Some(v) = first_env(&["PARLEY_TELNYX_PROFILE", "TELNYX_MESSAGING_PROFILE_ID"]) {
        cfg.telnyx_messaging_profile_id = v;
    }
    if let Some(v) = first_env(&["PARLEY_SMS_ENROLLMENT_PATH"]) {
        cfg.sms_enrollment_path = v;
    }
    if let Some(v) = first_env(&["PARLEY_SMS_CAMPAIGN_ID"]) {
        cfg.sms_campaign_id = v;
    }
    if let Some(v) = first_env(&[
        "PARLEY_CLOUDFLARE_KEY",
        "CLOUDFLARE_KEY",
        "CLOUDFLARE_API_TOKEN",
    ]) {
        cfg.cloudflare_key = v;
    }
    if let Some(v) = first_env(&["PARLEY_CLOUDFLARE_ACCOUNT", "CLOUDFLARE_ACCOUNT_ID"]) {
        cfg.cloudflare_account_id = v;
    }
    if let Some(v) = first_env(&["PARLEY_HOSTNAME", "PARLEY_PUBLIC_HOSTNAME"]) {
        cfg.public_hostname = v.clone();
        if cfg.vapi_public_base.starts_with("http://127.0.0.1")
            || cfg.vapi_public_base.starts_with("http://localhost")
        {
            cfg.vapi_public_base = format!("https://{}", cfg.public_hostname);
        }
    }
    if let Some(v) = first_env(&["PARLEY_UCTP_PUBLIC", "PARLEY_UCTP_WS"]) {
        cfg.uctp_public_ws = v;
    }
    if let Some(v) = first_env(&["PARLEY_TUNNEL"]) {
        cfg.tunnel = v != "0" && !v.eq_ignore_ascii_case("false");
    } else if !cfg.cloudflare_key.is_empty() {
        cfg.tunnel = true;
    }
    if cfg.public_hostname.is_empty() && !cfg.cloudflare_key.is_empty() {
        cfg.public_hostname = "parley.rudeless.ai".into();
        if cfg.vapi_public_base.starts_with("http://127.0.0.1")
            || cfg.vapi_public_base.starts_with("http://localhost")
        {
            cfg.vapi_public_base = format!("https://{}", cfg.public_hostname);
        }
    }
    if let Some(v) = first_env(&["PARLEY_PROVISION"]) {
        cfg.provision = v != "0" && !v.eq_ignore_ascii_case("false");
    }
}

fn first_env(names: &[&str]) -> Option<String> {
    for name in names {
        if let Ok(v) = std::env::var(name) {
            let v = v.trim().trim_matches('"').trim_matches('\'').trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn default_bind_http() -> String {
    "127.0.0.1:8080".into()
}
fn default_bind_uctp() -> String {
    "127.0.0.1:7443".into()
}
fn default_bind_sip() -> String {
    "127.0.0.1:5060".into()
}
fn default_sqlite() -> String {
    "parley.sqlite".into()
}
fn default_blob() -> String {
    "var/blobs".into()
}
fn default_tenant() -> String {
    "ten_local".into()
}
fn default_vapi_public() -> String {
    "http://127.0.0.1:8080".into()
}
fn default_reopen() -> i64 {
    604800
}
fn default_max_voice() -> u32 {
    16
}
fn default_vapi_chat() -> String {
    "live".into()
}
fn default_consent() -> String {
    "This conversation may be recorded.".into()
}
fn default_provision() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_secrets() {
        let mut cfg = Config::default();
        cfg.api_secret = "s3cret".into();
        cfg.vapi_api_key = "sk-live-not-for-logs".into();
        cfg.telnyx_api_key = "KEY0123".into();
        cfg.cloudflare_key = "cf-token".into();
        let debug = format!("{cfg:?}");
        assert!(!debug.contains("s3cret"));
        assert!(!debug.contains("sk-live-not-for-logs"));
        assert!(!debug.contains("KEY0123"));
        assert!(!debug.contains("cf-token"));
        assert!(debug.contains("<redacted>"));
    }
}
