use crate::error::ApiError;
use crate::http::Auth;
use crate::pickup::{self, AcceptPickup};
use crate::runtime::AppState;
use axum::extract::{Path, State};
use axum::Json;
use serde_json::{json, Value};

pub async fn request(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(cid): Path<String>,
) -> Result<Json<Value>, ApiError> {
    pickup::request(&state, &auth.tenant_id, &cid, None).await?;
    Ok(Json(json!({ "state": "requested" })))
}

pub async fn accept(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(cid): Path<String>,
    Json(body): Json<AcceptPickup>,
) -> Result<Json<Value>, ApiError> {
    let result = pickup::accept(&state, &auth.tenant_id, &cid, body).await?;
    Ok(Json(result))
}

pub async fn return_to_ai(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(cid): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let result = pickup::return_to_ai(&state, &auth.tenant_id, &cid).await?;
    Ok(Json(result))
}
