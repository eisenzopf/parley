use crate::conversation::{self, PostMessage};
use crate::error::ApiError;
use crate::http::Auth;
use crate::runtime::AppState;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use serde_json::{json, Value};

pub async fn create(
    State(state): State<AppState>,
    Auth(auth): Auth,
    headers: HeaderMap,
    Path(cid): Path<String>,
    Json(body): Json<PostMessage>,
) -> Result<Json<Value>, ApiError> {
    let idem = headers.get("idempotency-key").and_then(|v| v.to_str().ok());
    let row = conversation::post_message(&state, &auth.tenant_id, &cid, body, idem)?;
    crate::vapi_chat::reply_after(&state, &row).await;
    Ok(Json(serde_json::to_value(row).unwrap_or(json!({}))))
}

pub async fn list(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(cid): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let rows = state.store.list_messages(&auth.tenant_id, &cid)?;
    Ok(Json(json!({ "messages": rows })))
}
