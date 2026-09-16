use crate::error::ApiError;
use crate::http::Auth;
use crate::runtime::AppState;
use axum::extract::{Path, State};
use axum::Json;
use serde_json::{json, Value};

pub async fn list(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(cid): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let mut all = Vec::new();
    for session in state.store.list_sessions(&auth.tenant_id, &cid)? {
        all.extend(state.store.list_connections(&auth.tenant_id, &session.id)?);
    }
    Ok(Json(json!({ "connections": all })))
}

pub async fn list_for_session(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(sid): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let rows = state.store.list_connections(&auth.tenant_id, &sid)?;
    Ok(Json(json!({ "connections": rows })))
}

pub async fn get_one(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let row = state
        .store
        .get_connection(&auth.tenant_id, &id)?
        .ok_or_else(|| ApiError::not_found(format!("connection {id} not found")))?;
    Ok(Json(serde_json::to_value(row).unwrap_or(json!({}))))
}
