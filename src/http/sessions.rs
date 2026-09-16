use crate::conversation::{self, PostSession};
use crate::error::ApiError;
use crate::http::Auth;
use crate::runtime::AppState;
use axum::extract::{Path, State};
use axum::Json;
use serde_json::{json, Value};

pub async fn create(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(cid): Path<String>,
    Json(body): Json<PostSession>,
) -> Result<Json<Value>, ApiError> {
    let row = conversation::start_session(&state, &auth.tenant_id, &cid, body).await?;
    Ok(Json(serde_json::to_value(row).unwrap_or(json!({}))))
}

pub async fn list(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(cid): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let rows = state.store.list_sessions(&auth.tenant_id, &cid)?;
    Ok(Json(json!({ "sessions": rows })))
}

pub async fn get_one(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(sid): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let row = state
        .store
        .get_session(&auth.tenant_id, &sid)?
        .ok_or_else(|| ApiError::not_found("session not found"))?;
    Ok(Json(serde_json::to_value(row).unwrap_or(json!({}))))
}

pub async fn end(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(sid): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let row = conversation::end_session(&state, &auth.tenant_id, &sid).await?;
    Ok(Json(serde_json::to_value(row).unwrap_or(json!({}))))
}
