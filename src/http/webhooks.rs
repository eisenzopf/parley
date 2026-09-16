use crate::error::ApiError;
use crate::http::Auth;
use crate::runtime::AppState;
use crate::sms::{self, InboundSms};
use crate::vapi_tools;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use serde_json::{json, Value};

pub async fn list(
    State(_state): State<AppState>,
    Auth(_auth): Auth,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "webhooks": [] })))
}

pub async fn create(
    State(_state): State<AppState>,
    Auth(_auth): Auth,
    Json(_body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "ok": true })))
}

pub async fn vapi_tools(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let result = vapi_tools::handle(&state, &headers, body).await?;
    Ok(Json(result))
}

pub async fn sms_inbound_dev(
    State(state): State<AppState>,
    Json(body): Json<InboundSms>,
) -> Result<Json<Value>, ApiError> {
    let row = sms::inbound(&state, body).await?;
    Ok(Json(serde_json::to_value(row).unwrap_or(json!({}))))
}

pub async fn sms_inbound(
    State(state): State<AppState>,
    Json(body): Json<InboundSms>,
) -> Result<Json<Value>, ApiError> {
    sms_inbound_dev(State(state), Json(body)).await
}
