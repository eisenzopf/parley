use crate::error::Result;
use crate::runtime::AppState;
use crate::store::ParticipantRow;
use rvoip_core::ids::ParticipantId;
use serde_json::json;

pub fn record_path(state: &AppState, tenant_id: &str, cid: &str, path: &str) -> Result<()> {
    let _ = state.store.insert_vcon(tenant_id, Some(cid), None, path)?;
    Ok(())
}

/// Voice Session start: a system observer records consent once per Conversation.
pub fn on_voice_session_started(
    state: &AppState,
    tenant_id: &str,
    cid: &str,
    sid: &str,
) -> Result<()> {
    let parts = state.store.list_participants(tenant_id, cid)?;
    if !parts.iter().any(|p| p.kind == "system") {
        state.store.insert_participant(&ParticipantRow {
            id: ParticipantId::new().to_string(),
            tenant_id: tenant_id.into(),
            conversation_id: cid.into(),
            kind: "system".into(),
            role: "observer".into(),
            identity_ref: None,
            display_name: Some("recording".into()),
            joined_at: chrono::Utc::now().to_rfc3339(),
            left_at: None,
        })?;
    }
    let already = state
        .store
        .list_events(tenant_id, cid)?
        .iter()
        .any(|e| e.event_type == "recording.consented");
    if !already {
        crate::events::emit(
            state,
            tenant_id,
            Some(cid),
            "recording.consented",
            json!({
                "sid": sid,
                "consent": state.config.recording_consent
            }),
        )?;
    }
    Ok(())
}
