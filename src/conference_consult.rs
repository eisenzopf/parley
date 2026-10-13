//! Private owner consultation before transferring the retained organizer call.
use crate::{runtime::AppState, store::conference::Member, ApiError, Result};
use rvoip_core::ids::{BridgeId, ConnectionId, SessionId};
use serde_json::{json, Value};

// Keep actual G.711 RTP flowing while Jeff waits, so carrier inactivity
// detection does not clear his retained call during a longer consultation.
pub(crate) struct HoldAudio {
    cancel: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<std::result::Result<(), &'static str>>,
}

async fn start_hold(state: &AppState, tenant: &str, sid: &str, remote: ConnectionId) -> Result<()> {
    use std::time::Duration;
    use tokio::time::Instant;
    if state.conference_holds.lock().await.contains_key(sid) {
        return Ok(());
    }
    let stream = state
        .orchestrator
        .wait_for_stream(
            remote,
            rvoip_core::stream::StreamSelector::new(rvoip_core::stream::StreamKind::Audio)
                .with_readiness(rvoip_core::stream::MediaReadiness::Bidirectional),
            Instant::now() + Duration::from_secs(2),
            Default::default(),
        )
        .await
        .map_err(|_| ApiError::internal("held telephone audio unavailable"))?;
    let mut silence =
        crate::conference_phone_prompt::PhonePrompt::silence(stream).map_err(ApiError::internal)?;
    let (cancel, mut cancelled) = tokio::sync::oneshot::channel();
    let store = state.store.clone();
    let tenant = tenant.to_string();
    let held_sid = sid.to_string();
    let task = tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_millis(20));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _=&mut cancelled=>break,
                _=tick.tick()=> {
                    let active = store.get_session(&tenant,&held_sid).ok().flatten().is_some_and(|s| s.state=="active");
                    let waiting = store.conference_voice_for_session(&tenant,&held_sid).ok().flatten().is_some_and(|v| v.ai_state=="held");
                    if !active || (!waiting && store.browser_consult(&tenant,&held_sid).ok().flatten().is_none()) {break;}
                    silence.send_next(Instant::now()+Duration::from_millis(100)).await?;
                }
            }
        }
        silence.stop(Instant::now() + Duration::from_secs(1)).await
    });
    state
        .conference_holds
        .lock()
        .await
        .insert(sid.to_string(), HoldAudio { cancel, task });
    Ok(())
}

/// The announcement has finished and its Vapi leg has closed. Jeff stays on
/// the same telephone connection, with neither AI nor private browser audio.
pub(crate) async fn hold_organizer(
    state: &AppState,
    tenant: &str,
    voice: &crate::store::conference::VoiceOperation,
) -> Result<()> {
    let remote = ConnectionId::from_string(&voice.remote_connection_id);
    if let Some(bridge) = voice.bridge_id.as_deref() {
        if state.orchestrator.bridge_peer_of(&remote).is_some() {
            state
                .orchestrator
                .unbridge_connections(BridgeId::from_string(bridge))
                .await
                .map_err(super::conference_voice::core_error)?;
        }
    }
    state.store.mark_conference_organizer_held(tenant, voice)?;
    start_hold(state, tenant, &voice.session_id, remote).await
}

pub(crate) async fn stop_hold(state: &AppState, sid: &str) -> Result<()> {
    if let Some(hold) = state.conference_holds.lock().await.remove(sid) {
        let _ = hold.cancel.send(());
        hold.task
            .await
            .map_err(|_| ApiError::internal("held audio task failed"))?
            .map_err(ApiError::internal)?;
    }
    Ok(())
}

// A tool request alone cannot turn an ambiguous or negative owner reply into consent.
pub fn transfer_consent(text: &str) -> bool {
    let words = text
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if words.iter().any(|w| {
        matches!(
            w.as_str(),
            "no" | "not" | "don't" | "dont" | "wait" | "stop" | "later" | "but"
        )
    }) {
        return false;
    }
    let normalized = words.join(" ");
    words.first().is_some_and(|w| {
        matches!(
            w.as_str(),
            "yes" | "yeah" | "yep" | "yup" | "sure" | "okay" | "ok" | "absolutely"
        )
    }) || [
        "go ahead",
        "please do",
        "put him through",
        "connect him",
        "connect jeff",
        "bring him in",
        "bridge him in",
        "transfer him",
    ]
    .iter()
    .any(|s| normalized.starts_with(s))
}

pub async fn begin(
    state: &AppState,
    tenant: &str,
    cid: &str,
    sid: &str,
    owner: &Member,
    connid: &str,
    request: &str,
) -> Result<Value> {
    let _guard = state.conference_handoff.lock().await;
    if owner.role != "owner" {
        return Err(ApiError::forbidden("only the owner can answer David"));
    }
    state
        .store
        .require_conference_browser(tenant, cid, sid, &owner.participant_id, connid)?;
    state
        .store
        .require_answered_conference_browser(tenant, connid)?;
    let voice = state
        .store
        .conference_voice_for_session(tenant, sid)?
        .filter(|v| v.conversation_id == cid && matches!(v.ai_state.as_str(), "attached" | "held"))
        .ok_or_else(|| ApiError::conflict("David must be attached before consultation"))?;
    let members = state.store.conference_members(tenant, cid)?;
    if !members
        .iter()
        .any(|m| m.participant_id == voice.target_participant_id && m.role == "organizer")
    {
        return Err(ApiError::conflict(
            "private consultation requires the organizer call",
        ));
    }
    let adapter = state
        .vapi_adapter
        .as_ref()
        .ok_or_else(|| ApiError::conflict("voice assistant unavailable"))?;
    let ai = ConnectionId::from_string(
        voice
            .ai_connection_id
            .as_deref()
            .ok_or_else(|| ApiError::conflict("assistant connection missing"))?,
    );
    let remote = ConnectionId::from_string(&voice.remote_connection_id);
    let browser = ConnectionId::from_string(connid);
    let old_bridge = BridgeId::from_string(
        voice
            .bridge_id
            .as_deref()
            .ok_or_else(|| ApiError::conflict("assistant bridge missing"))?,
    );
    state
        .orchestrator
        .wait_for_stream(
            browser.clone(),
            rvoip_core::stream::StreamSelector::new(rvoip_core::stream::StreamKind::Audio)
                .with_readiness(rvoip_core::stream::MediaReadiness::Bidirectional),
            tokio::time::Instant::now() + std::time::Duration::from_secs(10),
            Default::default(),
        )
        .await
        .map_err(|_| ApiError::conflict("browser audio is not ready"))?;
    // Unbridge acknowledges removal in both directions. Jeff remains connected,
    // but cannot hear the private browser conversation or be heard in it.
    if voice.ai_state != "held" {
        state
            .orchestrator
            .unbridge_connections(old_bridge)
            .await
            .map_err(super::conference_voice::core_error)?;
        // Realtime applies instructions at initialization. Start David's private
        // voice phase with its own initial prompt rather than a mid-call update.
        state.store.mark_conference_ai_ending(tenant, sid)?;
        let _ = state
            .orchestrator
            .end_connection(ai.clone(), rvoip_core::EndReason::Normal)
            .await;
    }
    let greeting = if owner.sms.is_some() {
        "Hey, I got a flight for you. I'll text you the info, but I've got Jeff on the other line. Can I connect him?"
    } else {
        "Hey, I got a flight for you. I'll send you the details, but I've got Jeff on the other line. Can I connect him?"
    };
    let definition = private_definition(state, &voice, greeting);
    let options=rvoip_vapi::VapiCallOptions::new(rvoip_vapi::VapiAssistant::transient(definition))
        .with_audio_format(rvoip_vapi::VapiAudioFormat::PcmS16Le16Khz)
        .with_name("Parley David private consult")
        .with_peer_failure_policy(rvoip_vapi::VapiPeerFailurePolicy::LeaveCallerConnected)
        .with_metadata(json!({"conversation_id":cid,"session_id":sid,"participant_id":voice.assistant_participant_id,"voice_profile":"conference-consult/1"}));
    let call = match adapter
        .attach_agent_for_participant(
            &state.orchestrator,
            browser,
            rvoip_core::ids::ParticipantId::from_string(&voice.assistant_participant_id),
            options,
        )
        .await
    {
        Ok(call) => call,
        Err(error) => {
            tracing::warn!(
                reason = error.diagnostic_class(),
                "private Vapi attachment failed"
            );
            let _ = super::conference_voice::terminate(state, &SessionId::from_string(sid)).await;
            return Err(ApiError::internal(
                "David could not start the private consultation",
            ));
        }
    };
    let private_ai = call.vapi_connection_id().to_string();
    let bridge = call.bridge_id().to_string();
    state
        .store
        .replace_consult_assistant(tenant, sid, &private_ai, &bridge)?;
    state.store.update_conference_browser(tenant,connid,"consulting",Some(request),json!({"bridge_id":bridge,"held_connid":voice.remote_connection_id,"assistant_connid":private_ai}))?;
    if let Err(error) = start_hold(state, tenant, sid, remote).await {
        let _ = super::conference_voice::terminate(state, &SessionId::from_string(sid)).await;
        return Err(error);
    }
    super::conference_voice::observe_call(state, voice.clone(), call, true);
    Ok(
        json!({"sid":sid,"connid":connid,"state":"consulting","held_connid":voice.remote_connection_id}),
    )
}

fn private_definition(
    state: &AppState,
    voice: &crate::store::conference::VoiceOperation,
    greeting: &str,
) -> Value {
    let prompt=format!("# Role and situation\nYou are David, Jonathan's AI assistant. You are now speaking privately to Jonathan in his browser. Jeff, the conference organizer, is on hold and cannot hear you. You arranged a replacement flight for Jonathan and Andrew. Jeff has a question for Jonathan about shortening his presentation because he is arriving late.\n# Style\nBe casual, friendly, informal, and a little playful. Use contractions, short replies, and varied everyday language. No repeated confirmations or formal service language.\n# Conversation\nYour first message gives the flight update and asks whether to connect Jeff. Wait for Jonathan's answer; do not repeat the introduction. If he clearly agrees, invoke connect_organizer once and stay silent. If he declines, says wait, or asks a question, keep him private and answer naturally. Never assume answering the browser is permission to connect Jeff. Do not ask Jeff for pickup or agree to shorten Jonathan's presentation. Only state itinerary facts actually supplied in the background.\n# Background\n{}",voice.purpose);
    json!({
        "name":"Parley conference David",
        "model":{"provider":"openai","model":state.config.conference_voice_model,"maxTokens":250,
            "messages":[{"role":"system","content":prompt}],"tools":[{
                "type":"function","async":false,"function":{"name":"connect_organizer",
                    "description":"After Jonathan clearly says yes to connecting Jeff, bridge the waiting organizer to this browser. Never invoke for no, wait, or a question. Stay silent after invoking.",
                    "parameters":{"type":"object","properties":{},"additionalProperties":false}},"messages":[{"type":"request-start","content":""}],
                "server":{"url":format!("{}/v1/vapi/conference-tools/{}/{}",state.config.public_http_base().trim_end_matches('/'),voice.session_id,crate::vapi_tools::conference_tool_signature(&state.config.vapi_api_key,&voice.session_id))}
            }]},
        "voice":{"provider":"openai","voiceId":"cedar"},"firstMessage":greeting,
        "endCallPhrases":[],"endCallFunctionEnabled":false,"silenceTimeoutSeconds":180,"maxDurationSeconds":600,
        "serverMessages":[],"clientMessages":["transcript","status-update","speech-update","tool-calls"]
    })
}

pub async fn connect(
    state: &AppState,
    tenant: &str,
    cid: &str,
    sid: &str,
    actor: &Member,
    request: &str,
) -> Result<Value> {
    let _guard = state.conference_handoff.lock().await;
    let voice = state
        .store
        .conference_voice_for_session(tenant, sid)?
        .filter(|v| v.conversation_id == cid)
        .ok_or_else(|| ApiError::not_found("voice Session missing"))?;
    if actor.role != "assistant" || actor.participant_id != voice.assistant_participant_id {
        return Err(ApiError::forbidden(
            "transfer requires this call's assistant",
        ));
    }
    let route = state
        .store
        .browser_consult(tenant, sid)?
        .ok_or_else(|| ApiError::conflict("owner must be speaking privately with David"))?;
    let (seq, reply) = state
        .store
        .latest_consult_reply(tenant, cid, sid, &route.participant_id)?
        .ok_or_else(|| ApiError::conflict("owner consent required"))?;
    if !transfer_consent(&reply)
        || !state
            .store
            .has_consult_transfer_intent(tenant, cid, sid, &actor.participant_id)?
    {
        return Err(ApiError::conflict(
            "owner has not agreed to connect the organizer",
        ));
    }
    let ai = ConnectionId::from_string(
        voice
            .ai_connection_id
            .as_deref()
            .ok_or_else(|| ApiError::conflict("assistant connection missing"))?,
    );
    let browser = ConnectionId::from_string(&route.connection_id);
    let remote = ConnectionId::from_string(&voice.remote_connection_id);
    stop_hold(state, sid).await?;
    let replaced = state
        .orchestrator
        .replace_bridge_destination(
            BridgeId::from_string(&route.bridge_id),
            browser,
            ai.clone(),
            remote.clone(),
        )
        .await;
    let replaced = match replaced {
        Ok(value) => value,
        Err(error) => {
            if start_hold(state, tenant, sid, remote).await.is_err() {
                let _ =
                    super::conference_voice::terminate(state, &SessionId::from_string(sid)).await;
            }
            return Err(super::conference_voice::core_error(error));
        }
    };
    state.store.update_conference_browser(tenant,&route.connection_id,"speaking",Some(request),json!({"retained_connid":voice.remote_connection_id,"retired_connid":ai.to_string(),"bridge_id":replaced.bridge_id.to_string(),"consent_seq":seq}))?;
    // The attachment's original peer was Jeff. LeaveCallerConnected prevents
    // retiring this now-detached AI connection from hanging up his phone.
    let _ = state
        .orchestrator
        .end_connection(ai, rvoip_core::EndReason::Normal)
        .await;
    Ok(
        json!({"sid":sid,"connid":route.connection_id,"state":"speaking","retained_connid":voice.remote_connection_id,"consent_seq":seq}),
    )
}

#[cfg(test)]
mod tests {
    use super::transfer_consent;
    #[test]
    fn transfer_requires_a_clear_affirmative() {
        for text in [
            "Yes",
            "Yeah, go ahead!",
            "Sure, connect Jeff.",
            "Put him through",
            "Okay",
        ] {
            assert!(transfer_consent(text), "{text}");
        }
        for text in [
            "",
            "No",
            "Not yet",
            "Yes, but wait",
            "Don't connect him",
            "Can you tell me the flight first?",
            "Maybe later",
            "He said yes",
        ] {
            assert!(!transfer_consent(text), "{text}");
        }
    }
}
