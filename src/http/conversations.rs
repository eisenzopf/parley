use crate::conversation::{self, CreateConversation};
use crate::error::ApiError;
use crate::http::Auth;
use crate::runtime::AppState;
use axum::extract::{Path, State};
use axum::Json;
use serde_json::{json, Value};

pub async fn create(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Json(body): Json<CreateConversation>,
) -> Result<Json<Value>, ApiError> {
    let view = conversation::create_or_continue(&state, &auth.tenant_id, body).await?;
    Ok(Json(serde_json::to_value(view).unwrap_or(json!({}))))
}

pub async fn list(
    State(state): State<AppState>,
    Auth(auth): Auth,
) -> Result<Json<Value>, ApiError> {
    let rows = state.store.list_conversations(&auth.tenant_id)?;
    Ok(Json(json!({ "conversations": rows })))
}

pub async fn get_one(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(cid): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let view = conversation::view(&state, &auth.tenant_id, &cid, "get")?;
    Ok(Json(serde_json::to_value(view).unwrap_or(json!({}))))
}

pub async fn close(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(cid): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let view = conversation::close(&state, &auth.tenant_id, &cid).await?;
    Ok(Json(serde_json::to_value(view).unwrap_or(json!({}))))
}

pub async fn timeline(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(cid): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let tl = conversation::timeline(&state, &auth.tenant_id, &cid)?;
    Ok(Json(serde_json::to_value(tl).unwrap_or(json!({}))))
}

pub async fn vcon(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(cid): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let path = state.store.get_vcon_path(&auth.tenant_id, &cid)?;
    match path {
        Some(p) => {
            let body = std::fs::read_to_string(&p).unwrap_or_else(|_| "{}".into());
            let value: Value = serde_json::from_str(&body).unwrap_or(json!({ "path": p }));
            Ok(Json(value))
        }
        None => Err(ApiError::not_found("vcon not wrapped yet")),
    }
}
