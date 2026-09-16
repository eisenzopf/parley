use crate::auth;
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

pub async fn get(
    State(state): State<AppState>,
    Auth(auth): Auth,
) -> Result<Json<Value>, ApiError> {
    require_server(&auth)?;
    Ok(Json(json!({
        "id": state.config.tenant_id,
        "bind_http": state.config.bind_http,
        "bind_uctp_ws": state.config.bind_uctp_ws
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
