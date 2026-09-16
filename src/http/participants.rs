use crate::error::ApiError;
use crate::http::Auth;
use crate::runtime::AppState;
use axum::extract::{Path, State};
use axum::Json;
use rvoip_core::ids::{ParticipantId, SessionId};
use rvoip_core::participant::{ParticipantKind, ParticipantRole};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
pub struct AddParticipant {
    pub kind: String,
    pub role: String,
    pub display_name: Option<String>,
}

#[derive(Deserialize)]
pub struct PatchParticipant {
    pub role: Option<String>,
}

#[derive(Deserialize)]
pub struct RoleTransfer {
    pub to_participant_id: String,
    pub session_id: Option<String>,
}

pub async fn list(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(cid): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let rows = state.store.list_participants(&auth.tenant_id, &cid)?;
    Ok(Json(json!({ "participants": rows })))
}

pub async fn add(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(cid): Path<String>,
    Json(body): Json<AddParticipant>,
) -> Result<Json<Value>, ApiError> {
    let now = chrono::Utc::now().to_rfc3339();
    let row = crate::store::ParticipantRow {
        id: ParticipantId::new().to_string(),
        tenant_id: auth.tenant_id.clone(),
        conversation_id: cid,
        kind: body.kind,
        role: body.role,
        identity_ref: None,
        display_name: body.display_name,
        joined_at: now,
        left_at: None,
    };
    state.store.insert_participant(&row)?;
    Ok(Json(serde_json::to_value(row).unwrap_or(json!({}))))
}

pub async fn add_to_session(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(sid): Path<String>,
    Json(body): Json<AddParticipant>,
) -> Result<Json<Value>, ApiError> {
    let session = state
        .store
        .get_session(&auth.tenant_id, &sid)?
        .ok_or_else(|| ApiError::not_found("session not found"))?;
    let pid = ParticipantId::new();
    let kind = parse_kind(&body.kind)?;
    let role = parse_role(&body.role)?;
    state
        .orchestrator
        .join_session(SessionId::from_string(sid), pid.clone(), kind, role)
        .await
        .map_err(|e| ApiError::internal(format!("join_session: {e}")))?;
    let row = crate::store::ParticipantRow {
        id: pid.to_string(),
        tenant_id: auth.tenant_id,
        conversation_id: session.conversation_id,
        kind: body.kind,
        role: body.role,
        identity_ref: None,
        display_name: body.display_name,
        joined_at: chrono::Utc::now().to_rfc3339(),
        left_at: None,
    };
    state.store.insert_participant(&row)?;
    Ok(Json(serde_json::to_value(row).unwrap_or(json!({}))))
}

pub async fn hand_off(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(pid): Path<String>,
    Json(body): Json<RoleTransfer>,
) -> Result<Json<Value>, ApiError> {
    let from = state
        .store
        .get_participant(&auth.tenant_id, &pid)?
        .ok_or_else(|| ApiError::not_found("participant not found"))?;
    let sid = body
        .session_id
        .ok_or_else(|| ApiError::bad_request("session_id required"))?;
    state
        .orchestrator
        .hand_off(
            SessionId::from_string(sid),
            ParticipantId::from_string(pid.clone()),
            ParticipantId::from_string(body.to_participant_id.clone()),
            ParticipantKind::Human,
        )
        .await
        .map_err(|e| ApiError::internal(format!("hand_off: {e}")))?;
    state
        .store
        .set_participant_role(&auth.tenant_id, &pid, "observer")?;
    state.store.set_participant_role(
        &auth.tenant_id,
        &body.to_participant_id,
        "agent",
    )?;
    Ok(Json(json!({
        "conversation_id": from.conversation_id,
        "from": pid,
        "to": body.to_participant_id
    })))
}

pub async fn take_over(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(pid): Path<String>,
    Json(body): Json<RoleTransfer>,
) -> Result<Json<Value>, ApiError> {
    let from = state
        .store
        .get_participant(&auth.tenant_id, &pid)?
        .ok_or_else(|| ApiError::not_found("participant not found"))?;
    let sid = body
        .session_id
        .ok_or_else(|| ApiError::bad_request("session_id required"))?;
    state
        .orchestrator
        .take_over(
            SessionId::from_string(sid),
            ParticipantId::from_string(pid.clone()),
            ParticipantKind::Human,
        )
        .await
        .map_err(|e| ApiError::internal(format!("take_over: {e}")))?;
    state
        .store
        .set_participant_role(&auth.tenant_id, &pid, "agent")?;
    Ok(Json(json!({
        "conversation_id": from.conversation_id,
        "participant_id": pid
    })))
}

pub async fn patch(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(pid): Path<String>,
    Json(body): Json<PatchParticipant>,
) -> Result<Json<Value>, ApiError> {
    let row = state
        .store
        .get_participant(&auth.tenant_id, &pid)?
        .ok_or_else(|| ApiError::not_found("participant not found"))?;
    if let Some(role) = body.role.as_deref() {
        let orch_role = parse_role(role)?;
        state
            .orchestrator
            .set_participant_role(ParticipantId::from_string(pid.clone()), orch_role)
            .await
            .map_err(|e| ApiError::internal(format!("set_participant_role: {e}")))?;
        state
            .store
            .set_participant_role(&auth.tenant_id, &pid, role)?;
    }
    let row = state
        .store
        .get_participant(&auth.tenant_id, &pid)?
        .unwrap_or(row);
    Ok(Json(serde_json::to_value(row).unwrap_or(json!({}))))
}

pub async fn leave(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(pid): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let row = state
        .store
        .get_participant(&auth.tenant_id, &pid)?
        .ok_or_else(|| ApiError::not_found("participant not found"))?;
    Ok(Json(json!({ "id": row.id, "left": true })))
}

fn parse_kind(kind: &str) -> Result<ParticipantKind, ApiError> {
    Ok(match kind {
        "human" => ParticipantKind::Human,
        "ai" => ParticipantKind::Ai,
        "system" => ParticipantKind::System,
        "external" => ParticipantKind::External,
        _ => return Err(ApiError::bad_request("unknown participant kind")),
    })
}

fn parse_role(role: &str) -> Result<ParticipantRole, ApiError> {
    Ok(match role {
        "customer" => ParticipantRole::Customer,
        "agent" => ParticipantRole::Agent,
        "supervisor" => ParticipantRole::Supervisor,
        "observer" => ParticipantRole::Observer,
        other => ParticipantRole::Custom(other.into()),
    })
}
