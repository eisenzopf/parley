use crate::config::{hash_secret, keyed_mac, Config};
use crate::error::{ApiError, Result};
use crate::store::Store;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub enum Actor {
    ApiSecret,
    Widget { visitor_id: Option<String>, origin: Option<String> },
    Operator { operator_id: String },
}

#[derive(Clone, Debug)]
pub struct AuthContext {
    pub tenant_id: String,
    pub actor: Actor,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct WidgetClaims {
    tenant_id: String,
    visitor_id: Option<String>,
    origin: Option<String>,
    exp: i64,
}

pub fn authenticate(store: &Store, config: &Config, authorization: Option<&str>) -> Result<AuthContext> {
    let token = authorization
        .and_then(|h| h.strip_prefix("Bearer "))
        .or(authorization)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ApiError::unauthorized("missing bearer token"))?;

    if token == config.api_secret
        || hash_secret(token) == config.hash_api_secret()
    {
        return Ok(AuthContext {
            tenant_id: config.tenant_id.clone(),
            actor: Actor::ApiSecret,
        });
    }

    if let Some(ctx) = decode_widget_token(config, token)? {
        return Ok(ctx);
    }

    if let Some(ctx) = lookup_operator(store, token)? {
        return Ok(ctx);
    }

    Err(ApiError::unauthorized("invalid bearer token"))
}

pub fn mint_widget_token(
    config: &Config,
    visitor_id: Option<String>,
    origin: Option<String>,
) -> Result<String> {
    let claims = WidgetClaims {
        tenant_id: config.tenant_id.clone(),
        visitor_id,
        origin,
        exp: (Utc::now() + Duration::hours(1)).timestamp(),
    };
    let payload = serde_json::to_vec(&claims)
        .map_err(|e| ApiError::internal(format!("widget token: {e}")))?;
    let payload_hex = hex_encode(&payload);
    let mac = keyed_mac(&config.api_secret, payload_hex.as_bytes());
    Ok(format!("p1.{payload_hex}.{mac}"))
}

fn decode_widget_token(config: &Config, token: &str) -> Result<Option<AuthContext>> {
    let Some(rest) = token.strip_prefix("p1.") else {
        return Ok(None);
    };
    let mut parts = rest.splitn(2, '.');
    let Some(payload_hex) = parts.next() else {
        return Ok(None);
    };
    let Some(mac) = parts.next() else {
        return Err(ApiError::unauthorized("malformed widget token"));
    };
    let expected = keyed_mac(&config.api_secret, payload_hex.as_bytes());
    if expected != mac {
        return Err(ApiError::unauthorized("invalid widget token"));
    }
    let bytes = hex_decode(payload_hex).ok_or_else(|| ApiError::unauthorized("invalid widget token"))?;
    let claims: WidgetClaims = serde_json::from_slice(&bytes)
        .map_err(|_| ApiError::unauthorized("invalid widget token"))?;
    if claims.exp < Utc::now().timestamp() {
        return Err(ApiError::unauthorized("widget token expired"));
    }
    if claims.tenant_id != config.tenant_id {
        return Err(ApiError::unauthorized("widget token tenant mismatch"));
    }
    Ok(Some(AuthContext {
        tenant_id: claims.tenant_id,
        actor: Actor::Widget {
            visitor_id: claims.visitor_id,
            origin: claims.origin,
        },
    }))
}

fn lookup_operator(store: &Store, token: &str) -> Result<Option<AuthContext>> {
    let hash = hash_secret(token);
    let Some((tenant_id, operator_id, expires_at)) = store.lookup_operator_session(&hash)? else {
        return Ok(None);
    };
    let exp = chrono::DateTime::parse_from_rfc3339(&expires_at)
        .map_err(|_| ApiError::unauthorized("invalid operator session"))?;
    if exp < Utc::now() {
        return Err(ApiError::unauthorized("operator session expired"));
    }
    Ok(Some(AuthContext {
        tenant_id,
        actor: Actor::Operator { operator_id },
    }))
}

pub fn bootstrap_operator(
    store: &Store,
    config: &Config,
    token: &str,
    email: &str,
) -> Result<(String, String)> {
    if config.operator_bootstrap_token.is_empty() || token != config.operator_bootstrap_token {
        return Err(ApiError::unauthorized("invalid bootstrap token"));
    }
    if store.operator_count(&config.tenant_id)? > 0 {
        return Err(ApiError::conflict("operators already exist"));
    }
    let operator_id = store.insert_operator(&config.tenant_id, email)?;
    let session = format!("ops_{}", Uuid::new_v4().simple());
    store.insert_operator_session(
        &config.tenant_id,
        &operator_id,
        &hash_secret(&session),
        Utc::now() + Duration::hours(12),
    )?;
    Ok((operator_id, session))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}
