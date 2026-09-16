use serde::{Deserialize, Serialize};
use std::env;
use std::path::{Path, PathBuf};

use crate::error::{ApiError, Result};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub bind_http: String,
    pub bind_uctp_ws: String,
    pub bind_sip: String,
    pub sqlite_path: String,
    pub blob_dir: String,
    pub tenant_id: String,
    pub api_secret: String,
    pub operator_bootstrap_token: String,
    pub vapi_api_key: String,
    pub vapi_assistant_id: String,
    pub vapi_public_base: String,
    pub sip_advertise: String,
    pub reopen_window_secs: i64,
    pub max_voice_sessions_per_tenant: u32,
    #[serde(default)]
    pub ai_stub: bool,
    #[serde(default = "default_vapi_chat_mode")]
    pub vapi_chat_mode: String,
    #[serde(default)]
    pub hours: crate::hours::WeeklyHours,
    #[serde(default)]
    pub recording_consent: String,
}

fn default_vapi_chat_mode() -> String {
    "live".into()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind_http: "127.0.0.1:8080".into(),
            bind_uctp_ws: "127.0.0.1:7443".into(),
            bind_sip: "127.0.0.1:5060".into(),
            sqlite_path: "parley.sqlite".into(),
            blob_dir: "var/blobs".into(),
            tenant_id: "ten_local".into(),
            api_secret: "dev-only".into(),
            operator_bootstrap_token: String::new(),
            vapi_api_key: String::new(),
            vapi_assistant_id: String::new(),
            vapi_public_base: "http://127.0.0.1:8080".into(),
            sip_advertise: String::new(),
            reopen_window_secs: 604800,
            max_voice_sessions_per_tenant: 16,
            ai_stub: false,
            vapi_chat_mode: default_vapi_chat_mode(),
            hours: crate::hours::WeeklyHours::default(),
            recording_consent: "This conversation may be recorded.".into(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        Self::load_from_path(Path::new("config/default.toml"))
    }

    pub fn load_from_path(path: &Path) -> Result<Self> {
        let mut cfg = if path.exists() {
            let text = std::fs::read_to_string(path).map_err(|e| {
                ApiError::internal(format!("read config {}: {e}", path.display()))
            })?;
            toml::from_str(&text)
                .map_err(|e| ApiError::internal(format!("parse config: {e}")))?
        } else {
            Self::default()
        };
        overlay_env(&mut cfg);
        Ok(cfg)
    }

    pub fn sqlite_path_buf(&self) -> PathBuf {
        PathBuf::from(&self.sqlite_path)
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
    if let Ok(v) = env::var("PARLEY_BIND_HTTP") {
        cfg.bind_http = v;
    }
    if let Ok(v) = env::var("PARLEY_BIND_UCTP_WS") {
        cfg.bind_uctp_ws = v;
    }
    if let Ok(v) = env::var("PARLEY_BIND_SIP") {
        cfg.bind_sip = v;
    }
    if let Ok(v) = env::var("PARLEY_SQLITE_PATH") {
        cfg.sqlite_path = v;
    }
    if let Ok(v) = env::var("PARLEY_BLOB_DIR") {
        cfg.blob_dir = v;
    }
    if let Ok(v) = env::var("PARLEY_TENANT_ID") {
        cfg.tenant_id = v;
    }
    if let Ok(v) = env::var("PARLEY_API_SECRET") {
        cfg.api_secret = v;
    }
    if let Ok(v) = env::var("PARLEY_OPERATOR_BOOTSTRAP") {
        cfg.operator_bootstrap_token = v;
    }
    if let Ok(v) = env::var("PARLEY_VAPI_API_KEY") {
        cfg.vapi_api_key = v;
    }
    if let Ok(v) = env::var("PARLEY_VAPI_ASSISTANT_ID") {
        cfg.vapi_assistant_id = v;
    }
    if let Ok(v) = env::var("PARLEY_VAPI_PUBLIC_BASE") {
        cfg.vapi_public_base = v;
    }
    if let Ok(v) = env::var("PARLEY_SIP_ADVERTISE") {
        cfg.sip_advertise = v;
    }
    if let Ok(v) = env::var("PARLEY_REOPEN_WINDOW_SECS") {
        if let Ok(n) = v.parse() {
            cfg.reopen_window_secs = n;
        }
    }
    if env::var("PARLEY_AI_STUB").ok().as_deref() == Some("1") {
        cfg.ai_stub = true;
    }
    if let Ok(v) = env::var("PARLEY_VAPI_CHAT") {
        cfg.vapi_chat_mode = v;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_not_in_debug_via_hash_helper() {
        let h = hash_secret("dev-only");
        assert_ne!(h, "dev-only");
        assert_eq!(h.len(), 64);
    }
}
