//! Voice attach. Uses `rvoip-vapi::attach_agent_for_participant` so the AI
//! Participant is never the caller's id (UP-1).

use crate::error::{ApiError, Result};
use crate::runtime::AppState;
use rvoip_core::ids::ParticipantId;

pub struct VoiceAttach {
    pub ai_participant_id: String,
    pub customer_participant_id: String,
}

pub fn ai_and_customer(state: &AppState, tenant_id: &str, cid: &str) -> Result<VoiceAttach> {
    let parts = state.store.list_participants(tenant_id, cid)?;
    let customer = parts
        .iter()
        .find(|p| p.kind == "human" && p.role == "customer")
        .ok_or_else(|| ApiError::not_found("customer participant"))?;
    let ai = parts
        .iter()
        .find(|p| p.kind == "ai")
        .ok_or_else(|| ApiError::not_found("ai participant"))?;
    if customer.id == ai.id {
        return Err(ApiError::internal("ai participant must be distinct"));
    }
    let _ = ParticipantId::from_string(ai.id.clone());
    Ok(VoiceAttach {
        ai_participant_id: ai.id.clone(),
        customer_participant_id: customer.id.clone(),
    })
}

#[cfg(feature = "vapi")]
pub async fn attach(
    state: &AppState,
    tenant_id: &str,
    cid: &str,
    caller_connection: rvoip_core::ids::ConnectionId,
    vapi: &rvoip_vapi::VapiAdapter,
    options: rvoip_vapi::VapiCallOptions,
) -> Result<rvoip_vapi::VapiAgentCall> {
    let ids = ai_and_customer(state, tenant_id, cid)?;
    vapi.attach_agent_for_participant(
        &state.orchestrator,
        caller_connection,
        ParticipantId::from_string(ids.ai_participant_id),
        options,
    )
    .await
    .map_err(|e| ApiError::internal(format!("attach_agent: {e}")))
}
