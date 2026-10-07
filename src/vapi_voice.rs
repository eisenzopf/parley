//! Voice attach. Uses `rvoip-vapi::attach_agent_for_participant` so the AI
//! Participant is never the caller's id (UP-1).

use crate::error::{ApiError, Result};
use crate::runtime::AppState;
use rvoip_core::ids::{ConnectionId, ParticipantId};

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

pub fn spawn_attach_on_connected(state: AppState, connection_id: ConnectionId) {
    #[cfg(feature = "vapi")]
    {
        if state.vapi_adapter.is_none() || state.config.vapi_assistant_id.is_empty() {
            return;
        }
        tokio::spawn(async move {
            if let Err(e) = attach_live(&state, connection_id).await {
                tracing::warn!(error = %e, "vapi voice attach skipped");
            }
        });
    }
    #[cfg(not(feature = "vapi"))]
    {
        let _ = (state, connection_id);
    }
}

#[cfg(feature = "vapi")]
async fn attach_live(state: &AppState, caller_connection: ConnectionId) -> Result<()> {
    use rvoip_core::participant::ParticipantKind;
    use rvoip_core::session::SessionMedium;
    use rvoip_vapi::{VapiAssistant, VapiAudioFormat, VapiCallOptions};

    if let Some(voice) = state
        .store
        .conference_voice_for_connection(&state.config.tenant_id, &caller_connection.to_string())?
    {
        return crate::conference_voice::attach_vapi(state, voice).await;
    }

    let adapter = state
        .vapi_adapter
        .clone()
        .ok_or_else(|| ApiError::internal("vapi adapter missing"))?;
    let sid = state
        .orchestrator
        .session_of(&caller_connection)
        .ok_or_else(|| ApiError::not_found("connection has no session"))?;
    let cid = {
        let session = state
            .orchestrator
            .session(&sid)
            .ok_or_else(|| ApiError::not_found("session missing"))?;
        let session = session
            .read()
            .map_err(|_| ApiError::internal("session lock"))?;
        if session.medium != SessionMedium::Voice {
            return Ok(());
        }
        if let Some(cref) = session.connections.get(&caller_connection) {
            if let Some(conv) = state.orchestrator.conversation(&session.conversation_id) {
                if let Ok(conv) = conv.read() {
                    if conv
                        .participants
                        .iter()
                        .any(|p| p.id == cref.participant_id && p.kind == ParticipantKind::Ai)
                    {
                        return Ok(());
                    }
                }
            }
        }
        session.conversation_id.to_string()
    };
    // Conference attachment is explicit and only follows its invited remote
    // Connection. A browser connecting must never auto-create another Vapi call.
    if !state
        .store
        .conference_members(&state.config.tenant_id, &cid)?
        .is_empty()
    {
        return Ok(());
    }
    {
        let calls = state.vapi_calls.lock().expect("vapi calls");
        if calls.contains_key(&sid.to_string()) {
            return Ok(());
        }
    }
    let ids = ai_and_customer(state, &state.config.tenant_id, &cid)?;
    let options =
        VapiCallOptions::new(VapiAssistant::saved(state.config.vapi_assistant_id.clone()))
            .with_audio_format(VapiAudioFormat::PcmS16Le16Khz)
            .with_name("Parley");
    let call = adapter
        .attach_agent_for_participant(
            &state.orchestrator,
            caller_connection,
            ParticipantId::from_string(ids.ai_participant_id.clone()),
            options,
        )
        .await
        .map_err(|e| ApiError::internal(format!("attach_agent: {e}")))?;
    state
        .vapi_calls
        .lock()
        .expect("vapi calls")
        .insert(sid.to_string(), call.vapi_connection_id().clone());
    tracing::info!(
        cid = %cid,
        session = %sid,
        ai = %ids.ai_participant_id,
        "vapi voice attached as ai participant"
    );
    Ok(())
}

pub async fn mute_session(state: &AppState, sid: &str) {
    #[cfg(feature = "vapi")]
    {
        let conn = state
            .vapi_calls
            .lock()
            .expect("vapi calls")
            .get(sid)
            .cloned();
        if let (Some(adapter), Some(conn)) = (state.vapi_adapter.as_ref(), conn) {
            if let Err(e) = adapter.mute_assistant(&conn).await {
                tracing::warn!(error = %e, session = %sid, "vapi mute skipped");
            }
        }
    }
    #[cfg(not(feature = "vapi"))]
    {
        let _ = (state, sid);
    }
}

pub async fn unmute_session(state: &AppState, sid: &str) {
    #[cfg(feature = "vapi")]
    {
        let conn = state
            .vapi_calls
            .lock()
            .expect("vapi calls")
            .get(sid)
            .cloned();
        if let (Some(adapter), Some(conn)) = (state.vapi_adapter.as_ref(), conn) {
            if let Err(e) = adapter.unmute_assistant(&conn).await {
                tracing::warn!(error = %e, session = %sid, "vapi unmute skipped");
            }
        }
    }
    #[cfg(not(feature = "vapi"))]
    {
        let _ = (state, sid);
    }
}
