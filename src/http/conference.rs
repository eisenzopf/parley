//! Administration only. Demo communication controls go through UCTP.
use super::Auth;
use crate::{runtime::AppState, ApiError};
use axum::{
    extract::{Path, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
pub struct TokenRequest {
    pub participant_id: String,
}

pub async fn token(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(cid): Path<String>,
    Json(body): Json<TokenRequest>,
) -> Result<Json<Value>, ApiError> {
    if !matches!(auth.actor, crate::auth::Actor::ApiSecret) {
        return Err(ApiError::forbidden(
            "conference provisioning requires administrator",
        ));
    }
    let token = state
        .store
        .issue_conference_token(&auth.tenant_id, &cid, &body.participant_id)?;
    Ok(Json(
        json!({"token":token,"conversation_id":cid,"participant_id":body.participant_id,"expires_in":43200}),
    ))
}
