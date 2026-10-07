use crate::auth;
use crate::config::WeeklyHours;
use crate::error::ApiError;
use crate::http::{require_server, Auth};
use crate::runtime::AppState;
use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
pub struct WidgetTokenRequest {
    pub visitor_id: Option<String>,
    pub origin: Option<String>,
}

#[derive(Deserialize)]
pub struct BootstrapRequest {
    pub token: String,
    pub email: String,
}

pub async fn public_config(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let mut body = crate::cloudflare_tunnel::widget_public_json(None, &state.config);
    body["uctp_ws_url"] = json!(state.live.uctp_ws_url);
    body["http_base"] = json!(state.live.public_base);
    if !state.config.telnyx_from.is_empty() {
        body["sms"] = json!(state.config.telnyx_from);
    }
    Ok(Json(body))
}

pub async fn public_widget_token(
    State(state): State<AppState>,
    Json(body): Json<WidgetTokenRequest>,
) -> Result<Json<Value>, ApiError> {
    let token = auth::mint_widget_token(&state.config, body.visitor_id, body.origin)?;
    Ok(Json(json!({ "token": token, "expires_in": 3600 })))
}

pub async fn get(State(state): State<AppState>, Auth(auth): Auth) -> Result<Json<Value>, ApiError> {
    require_server(&auth)?;
    Ok(Json(json!({
        "id": state.config.tenant_id,
        "bind_http": state.config.bind_http,
        "bind_uctp_ws": state.config.bind_uctp_ws,
        "hours": state.hours.lock().expect("hours").clone(),
    })))
}

pub async fn hours(
    State(state): State<AppState>,
    Auth(auth): Auth,
) -> Result<Json<Value>, ApiError> {
    require_server(&auth)?;
    let hours = state.hours.lock().expect("hours").clone();
    Ok(Json(serde_json::to_value(hours).unwrap_or(json!({}))))
}

pub async fn put_hours(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Json(hours): Json<WeeklyHours>,
) -> Result<Json<Value>, ApiError> {
    require_server(&auth)?;
    *state.hours.lock().expect("hours") = hours.clone();
    let mut cfg = state.store.tenant_config(&auth.tenant_id)?;
    cfg["hours"] = serde_json::to_value(&hours).unwrap_or(json!({}));
    state.store.set_tenant_config(&auth.tenant_id, &cfg)?;
    Ok(Json(serde_json::to_value(hours).unwrap_or(json!({}))))
}

pub async fn ai(State(state): State<AppState>, Auth(auth): Auth) -> Result<Json<Value>, ApiError> {
    require_server(&auth)?;
    Ok(Json(json!({
        "assistant_id": state.config.vapi_assistant_id,
        "chat_mode": state.config.vapi_chat_mode,
    })))
}

pub async fn pickup(
    State(state): State<AppState>,
    Auth(auth): Auth,
) -> Result<Json<Value>, ApiError> {
    require_server(&auth)?;
    Ok(Json(json!({
        "target": "browser",
        "tenant_id": state.config.tenant_id,
    })))
}

pub async fn widget(
    State(state): State<AppState>,
    Auth(auth): Auth,
) -> Result<Json<Value>, ApiError> {
    require_server(&auth)?;
    Ok(Json(json!({
        "uctp_ws_url": state.live.uctp_ws_url,
        "http_base": state.live.public_base,
    })))
}

pub async fn numbers(
    State(state): State<AppState>,
    Auth(auth): Auth,
) -> Result<Json<Value>, ApiError> {
    require_server(&auth)?;
    Ok(Json(json!({
        "telnyx_from": state.config.telnyx_from,
        "sip_advertise": state.config.sip_advertise,
    })))
}

pub async fn widget_token(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Json(body): Json<WidgetTokenRequest>,
) -> Result<Json<Value>, ApiError> {
    require_server(&auth)?;
    let token = auth::mint_widget_token(&state.config, body.visitor_id, body.origin)?;
    Ok(Json(json!({ "token": token, "expires_in": 3600 })))
}

pub async fn bootstrap(
    State(state): State<AppState>,
    Json(body): Json<BootstrapRequest>,
) -> Result<Json<Value>, ApiError> {
    let (operator_id, session) =
        auth::bootstrap_operator(&state.store, &state.config, &body.token, &body.email)?;
    Ok(Json(json!({
        "operator_id": operator_id,
        "session_token": session
    })))
}
