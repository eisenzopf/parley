//! Conference voice uses actual Orchestrator connections, never synthetic legs.
use crate::{runtime::AppState, store::conference::Member, ApiError, Result};
use rvoip_core::{
    events::Event,
    ids::{ConversationId, ParticipantId, SessionId},
    participant::{ParticipantKind, ParticipantRole},
    session::SessionMedium,
    Transport,
};
use serde_json::{json, Value};

pub fn available(state: &AppState) -> bool {
    state.orchestrator.adapter(Transport::Sip).is_ok()
}
pub fn assistant_available(state: &AppState) -> bool {
    #[cfg(feature = "vapi")]
    {
        state.vapi_adapter.is_some() && !state.config.vapi_assistant_id.is_empty()
    }
    #[cfg(not(feature = "vapi"))]
    {
        let _ = state;
        false
    }
}

pub async fn bind(state: &AppState) -> Result<()> {
    let Some(bind) = &state.config.conference_sip_bind else {
        return Ok(());
    };
    #[cfg(not(feature = "sip"))]
    {
        let _ = bind;
        return Err(ApiError::bad_request(
            "conference SIP requires the sip feature",
        ));
    }
    #[cfg(feature = "sip")]
    {
        let addr: std::net::SocketAddr = bind
            .parse()
            .map_err(|_| ApiError::bad_request("invalid CONFERENCE_SIP_BIND"))?;
        let config = state
            .config
            .conference_network
            .sip_config(addr, &state.config.conference_sip_from)?;
        let adapter = rvoip_sip::SipAdapter::from_config(config)
            .await
            .map_err(|e| ApiError::internal(format!("conference SIP adapter: {e}")))?;
        state
            .orchestrator
            .register(adapter)
            .map_err(|e| ApiError::internal(format!("register SIP: {e}")))?;
        tracing::info!(%addr, "conference SIP adapter listening");
        Ok(())
    }
}

pub async fn invite(
    state: &AppState,
    tenant: &str,
    cid: &str,
    actor: &Member,
    target: &Member,
    purpose: &str,
    request_id: &str,
) -> Result<Value> {
    if !actor.observes_all() {
        return Err(ApiError::forbidden(
            "voice invitation requires owner or assistant",
        ));
    }
    if purpose.trim().is_empty() || purpose.len() > 4000 {
        return Err(ApiError::bad_request(
            "voice purpose required, maximum 4000 bytes",
        ));
    }
    let endpoint = target
        .sip
        .as_deref()
        .ok_or_else(|| ApiError::conflict("participant has no provisioned voice route"))?;
    if !available(state) {
        return Err(ApiError::new(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "voice-unavailable",
            "Voice unavailable",
            "conference SIP adapter is not running",
        ));
    }
    let conv = state
        .store
        .get_conversation(tenant, cid)?
        .ok_or_else(|| ApiError::not_found("Conversation missing"))?;
    if conv.state != "open" || state.store.count_live_voice_sessions(tenant, cid)? > 0 {
        return Err(ApiError::conflict(
            "Conversation closed or voice Session already active",
        ));
    }
    let assistant = state
        .store
        .conference_members(tenant, cid)?
        .into_iter()
        .find(|m| m.role == "assistant")
        .ok_or_else(|| ApiError::conflict("assistant participant required"))?;
    if target.participant_id == assistant.participant_id {
        return Err(ApiError::bad_request(
            "voice target must be another participant",
        ));
    }
    state
        .orchestrator
        .open_conversation_with_id(
            ConversationId::from_string(cid),
            rvoip_core::ids::TenantId::from_string(tenant),
            rvoip_core::conversation::ConversationPolicy::Persistent,
            Default::default(),
        )
        .await
        .map_err(core_error)?;
    let sid = state
        .orchestrator
        .start_session(
            ConversationId::from_string(cid),
            SessionMedium::Voice,
            vec![],
        )
        .await
        .map_err(core_error)?;
    let result = async {
        for (member,kind,role) in [(target,ParticipantKind::Human,ParticipantRole::Customer),(&assistant,ParticipantKind::Ai,ParticipantRole::Agent)] {
            state.orchestrator.join_session(sid.clone(),ParticipantId::from_string(&member.participant_id),kind,role).await.map_err(core_error)?;
        }
        let ticket = state.orchestrator.prepare_outbound_connection(rvoip_core::OriginateRequest::new(
            sid.clone(),ParticipantId::from_string(&target.participant_id),endpoint,rvoip_core::Direction::Outbound,Default::default()
        ).with_transport(Transport::Sip)).await.map_err(core_error)?;
        let connid = ticket.connection_id().to_string();
        state.store.prepare_conference_voice(tenant,cid,&sid.to_string(),request_id,target,&assistant,&connid,purpose,&actor.participant_id)?;
        // The durable association exists before activation can emit events.
        // Dialing completion arrives through the journal, not this acceptance.
        let state = state.clone(); let session_id = sid.clone(); let connection_id = connid.clone(); let tenant = tenant.to_owned();
        tokio::spawn(async move {
            if let Err(error) = ticket.commit().await {
                tracing::warn!(%error, "conference outbound activation failed");
                let _ = state.store.conference_voice_connection_event(&tenant,&connection_id,"failed",json!({"reason":"outbound activation failed"}));
                let _ = terminate(&state,&session_id).await;
            }
        });
        Ok(json!({"sid":sid.to_string(),"connid":connid,"participant_id":target.participant_id,"state":"accepted"}))
    }.await;
    if result.is_err() {
        let _ = state
            .orchestrator
            .end_session(sid, rvoip_core::EndReason::Normal)
            .await;
    }
    result
}

pub async fn end(
    state: &AppState,
    tenant: &str,
    cid: &str,
    sid: &str,
    actor: &Member,
    request: &str,
) -> Result<Value> {
    let _handoff = state.conference_handoff.lock().await;
    if !actor.observes_all() {
        return Err(ApiError::forbidden(
            "ending voice requires owner or assistant",
        ));
    }
    let session = state
        .store
        .get_session(tenant, sid)?
        .filter(|s| s.conversation_id == cid)
        .ok_or_else(|| ApiError::not_found("Session not in this Conversation"))?;
    if session.state == "interrupted" {
        return Err(ApiError::conflict(
            "remote call termination is unknown after restart; owner verification required",
        ));
    }
    if session.state != "ended" && session.state != "failed" {
        state.store.mark_conference_ai_ending(tenant, sid)?;
        terminate(state, &SessionId::from_string(sid)).await?;
        state
            .store
            .finish_conference_voice(tenant, sid, "ended", Some(request))?;
    }
    Ok(json!({"sid":sid,"state":"ended"}))
}

pub fn mirror(state: &AppState, event: &Event) -> Result<()> {
    crate::conference_phone::mirror(state, event)?;
    let tenant = &state.config.tenant_id;
    let terminal = match event {
        Event::ConnectionOutbound { connection_id, .. } => {
            state.store.conference_voice_connection_event(
                tenant,
                &connection_id.to_string(),
                "dialing",
                json!({}),
            )?;
            None
        }
        Event::ConnectionProgress {
            connection_id,
            kind,
            ..
        } => {
            state.store.conference_voice_connection_event(
                tenant,
                &connection_id.to_string(),
                "progress",
                json!({"progress":format!("{kind:?}")}),
            )?;
            None
        }
        Event::ConnectionConnected { connection_id, .. } => {
            state.store.conference_voice_connection_event(
                tenant,
                &connection_id.to_string(),
                "connected",
                json!({}),
            )?;
            None
        }
        Event::ConnectionEnded {
            connection_id,
            reason,
            ..
        } => {
            // A person hanging up normally initiates paired AI shutdown. A
            // BridgeTorn/failed end caused by the AI must retain its failure.
            if matches!(reason, rvoip_core::EndReason::Normal) {
                if let Some(voice) = state
                    .store
                    .conference_voice_for_connection(tenant, &connection_id.to_string())?
                {
                    state
                        .store
                        .mark_conference_ai_ending(tenant, &voice.session_id)?;
                }
            }
            state.store.conference_voice_connection_event(
                tenant,
                &connection_id.to_string(),
                "ended",
                json!({"reason":format!("{reason:?}")}),
            )?;
            Some(connection_id)
        }
        Event::ConnectionFailed { connection_id, .. } => {
            state
                .store
                .record_conference_ai_failure(tenant, &connection_id.to_string())?;
            state.store.conference_voice_connection_event(
                tenant,
                &connection_id.to_string(),
                "failed",
                json!({"reason":"connection failed"}),
            )?;
            Some(connection_id)
        }
        Event::SessionEnded { session_id, .. } => {
            state
                .store
                .finish_conference_voice(tenant, &session_id.to_string(), "ended", None)?;
            None
        }
        Event::SessionFailed { session_id, .. } => {
            state
                .store
                .finish_conference_voice(tenant, &session_id.to_string(), "failed", None)?;
            None
        }
        _ => None,
    };
    if let Some(connection_id) = terminal {
        state.store.update_conference_browser(
            tenant,
            &connection_id.to_string(),
            "ended",
            None,
            json!({}),
        )?;
        if let Some(voice) = state
            .store
            .conference_voice_for_connection(tenant, &connection_id.to_string())?
        {
            let state = state.clone();
            tokio::spawn(async move {
                let _handoff = state.conference_handoff.lock().await;
                let _ = terminate(&state, &SessionId::from_string(voice.session_id)).await;
            });
        }
    }
    Ok(())
}

#[cfg(feature = "media-webrtc")]
pub async fn join_browser(
    state: &AppState,
    tenant: &str,
    cid: &str,
    sid: &str,
    member: &Member,
    request: &str,
) -> Result<Value> {
    if member.role != "owner" {
        return Err(ApiError::forbidden(
            "conference browser join requires the owner",
        ));
    }
    let voice = state
        .store
        .conference_voice_for_session(tenant, sid)?
        .filter(|v| v.conversation_id == cid)
        .ok_or_else(|| ApiError::not_found("voice Session not in this Conversation"))?;
    if voice.ai_state != "attached"
        || state
            .store
            .get_session(tenant, sid)?
            .is_none_or(|s| s.state != "active")
    {
        return Err(ApiError::conflict(
            "assistant must be attached to an active voice Session before handoff",
        ));
    }
    state
        .orchestrator
        .join_session(
            SessionId::from_string(sid),
            ParticipantId::from_string(&member.participant_id),
            ParticipantKind::Human,
            ParticipantRole::Supervisor,
        )
        .await
        .map_err(core_error)?;
    let ticket = state
        .orchestrator
        .prepare_outbound_connection(
            rvoip_core::OriginateRequest::new(
                SessionId::from_string(sid),
                ParticipantId::from_string(&member.participant_id),
                "uctp-browser",
                rvoip_core::Direction::Outbound,
                rvoip_core::adapter::ConnectionAdapter::capabilities(
                    state.conference_browser.as_ref(),
                ),
            )
            .with_transport(Transport::WebRtc),
        )
        .await
        .map_err(core_error)?;
    let connid = ticket.connection_id().to_string();
    state.store.prepare_conference_browser(
        tenant,
        &voice,
        &member.participant_id,
        &connid,
        request,
    )?;
    let connection = rvoip_core::ids::ConnectionId::from_string(&connid);
    if let Err(error) = ticket.commit().await {
        state.store.update_conference_browser(
            tenant,
            &connid,
            "failed",
            Some(request),
            json!({"reason":"browser activation failed"}),
        )?;
        return Err(core_error(error));
    }
    let sdp = match state.conference_browser.local_sdp(&connection) {
        Ok(sdp) => sdp,
        Err(error) => {
            let _ = state
                .orchestrator
                .end_connection(connection, rvoip_core::EndReason::Normal)
                .await;
            return Err(ApiError::internal(format!("browser offer: {error}")));
        }
    };
    // Unanswered/failed preparations expire independently of the UCTP socket.
    // A successful handoff keeps the media route for the Session lifetime.
    let cleanup = state.clone();
    let cleanup_conn = connid.clone();
    let cleanup_tenant = tenant.to_owned();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        let active = cleanup
            .orchestrator
            .bridge_peer_of(&rvoip_core::ids::ConnectionId::from_string(&cleanup_conn))
            .is_some();
        if !active {
            let _ = cleanup
                .orchestrator
                .end_connection(
                    rvoip_core::ids::ConnectionId::from_string(&cleanup_conn),
                    rvoip_core::EndReason::Normal,
                )
                .await;
            let _ = cleanup.store.update_conference_browser(
                &cleanup_tenant,
                &cleanup_conn,
                "failed",
                None,
                json!({"reason":"browser handoff preparation expired"}),
            );
        }
    });
    Ok(
        json!({"sid":sid,"connid":connid,"substrate":"webrtc","substrate_setup":{"sdp_type":"offer","sdp":sdp},"ice_servers":state.config.conference_network.browser_ice}),
    )
}

#[cfg(feature = "media-webrtc")]
pub async fn answer_browser(
    state: &AppState,
    tenant: &str,
    cid: &str,
    sid: &str,
    member: &Member,
    connid: &str,
    sdp: &str,
    request: &str,
) -> Result<Value> {
    state
        .store
        .require_conference_browser(tenant, cid, sid, &member.participant_id, connid)?;
    if sdp.len() > 131072 || !sdp.starts_with("v=0") {
        return Err(ApiError::bad_request("valid bounded answer SDP required"));
    }
    state
        .conference_browser
        .apply_remote_answer(rvoip_core::ids::ConnectionId::from_string(connid), sdp)
        .await
        .map_err(|e| ApiError::internal(format!("browser answer: {e}")))?;
    // A local-offer WebRTC route needs explicit acceptance after applying its
    // remote answer. This waits for ICE/DTLS and initializes media, rather than
    // treating the existence of SDP or a lazily allocated stream as readiness.
    rvoip_core::adapter::ConnectionAdapter::accept(
        state.conference_browser.as_ref(),
        rvoip_core::ids::ConnectionId::from_string(connid),
    )
    .await
    .map_err(core_error)?;
    state
        .store
        .update_conference_browser(tenant, connid, "answered", Some(request), json!({}))?;
    Ok(json!({"sid":sid,"connid":connid,"state":"answered"}))
}

#[cfg(feature = "media-webrtc")]
pub async fn handoff_browser(
    state: &AppState,
    tenant: &str,
    cid: &str,
    sid: &str,
    member: &Member,
    connid: &str,
    request: &str,
) -> Result<Value> {
    let _handoff = state.conference_handoff.lock().await;
    state
        .store
        .require_conference_browser(tenant, cid, sid, &member.participant_id, connid)?;
    state
        .store
        .require_answered_conference_browser(tenant, connid)?;
    rvoip_core::adapter::ConnectionAdapter::accept(
        state.conference_browser.as_ref(),
        rvoip_core::ids::ConnectionId::from_string(connid),
    )
    .await
    .map_err(core_error)?;
    let voice = state
        .store
        .conference_voice_for_session(tenant, sid)?
        .filter(|v| v.conversation_id == cid)
        .ok_or_else(|| ApiError::not_found("voice Session missing"))?;
    let bridge = voice
        .bridge_id
        .as_deref()
        .ok_or_else(|| ApiError::conflict("assistant bridge missing"))?;
    let ai = voice
        .ai_connection_id
        .as_deref()
        .ok_or_else(|| ApiError::conflict("assistant connection missing"))?;
    // Core prepares the replacement before committing it. Failure keeps the
    // original SIP/Vapi bridge; success retains the same organizer Connection.
    let replaced = state
        .orchestrator
        .replace_bridge_destination(
            rvoip_core::ids::BridgeId::from_string(bridge),
            rvoip_core::ids::ConnectionId::from_string(&voice.remote_connection_id),
            rvoip_core::ids::ConnectionId::from_string(ai),
            rvoip_core::ids::ConnectionId::from_string(connid),
        )
        .await
        .map_err(core_error)?;
    state.store.update_conference_browser(tenant,connid,"speaking",Some(request),json!({"retained_connid":voice.remote_connection_id,"retired_connid":ai,"bridge_id":replaced.bridge_id.to_string()}))?;
    Ok(
        json!({"sid":sid,"connid":connid,"retained_connid":voice.remote_connection_id,"bridge_id":replaced.bridge_id.to_string(),"state":"speaking"}),
    )
}

fn core_error(error: rvoip_core::RvoipError) -> ApiError {
    let reason = match &error {
        rvoip_core::RvoipError::InvalidState(reason) => *reason,
        rvoip_core::RvoipError::Adapter(reason)
            if reason.starts_with("WebRTC operation failed (class=") && reason.len() < 100 =>
        {
            reason.as_str()
        }
        _ => error.diagnostic_class(),
    };
    tracing::debug!(reason, "conference core operation failed");
    ApiError::internal(format!("conference voice: {error}"))
}

/// In 0.3.12 `end_session` finalizes the Session but does not call each
/// transport's end operation. Release real connections before finalizing.
pub(crate) async fn terminate(state: &AppState, sid: &SessionId) -> Result<()> {
    let session = state
        .orchestrator
        .session(sid)
        .ok_or_else(|| ApiError::not_found("voice Session missing"))?;
    let ids = session
        .read()
        .map_err(|_| ApiError::internal("session lock"))?
        .connections
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    for id in ids {
        match state
            .orchestrator
            .end_connection(id, rvoip_core::EndReason::Normal)
            .await
        {
            Ok(()) | Err(rvoip_core::RvoipError::ConnectionNotFound(_)) => {}
            Err(error) => return Err(core_error(error)),
        }
    }
    state
        .orchestrator
        .end_session(sid.clone(), rvoip_core::EndReason::Normal)
        .await
        .map_err(core_error)
}

#[cfg(feature = "vapi")]
pub async fn attach_vapi(
    state: &AppState,
    voice: crate::store::conference::VoiceOperation,
) -> Result<()> {
    use rvoip_vapi::{VapiAssistant, VapiAudioFormat, VapiCallOptions, VapiEvent};
    let tenant = &state.config.tenant_id;
    if !state.store.claim_conference_ai(tenant, &voice.session_id)? {
        return Ok(());
    }
    let adapter = state
        .vapi_adapter
        .clone()
        .ok_or_else(|| ApiError::conflict("Vapi voice adapter unavailable"))?;
    let members = state
        .store
        .conference_members(tenant, &voice.conversation_id)?;
    let target = members
        .iter()
        .find(|m| m.participant_id == voice.target_participant_id)
        .ok_or_else(|| ApiError::not_found("voice target missing"))?;
    let assistant = members
        .iter()
        .find(|m| m.participant_id == voice.assistant_participant_id)
        .ok_or_else(|| ApiError::not_found("voice assistant missing"))?;
    // SIP answer signaling may precede activation of its RTP stream. Wait for
    // both media directions before creating a provider call or taking ownership
    // of stream receivers; bridging an unactivated stream fails immediately.
    if let Err(error) = state
        .orchestrator
        .wait_for_stream(
            rvoip_core::ids::ConnectionId::from_string(&voice.remote_connection_id),
            rvoip_core::stream::StreamSelector::new(rvoip_core::stream::StreamKind::Audio)
                .with_readiness(rvoip_core::stream::MediaReadiness::Bidirectional),
            tokio::time::Instant::now() + std::time::Duration::from_secs(10),
            Default::default(),
        )
        .await
    {
        state.store.conference_voice_fact(
            tenant,
            &voice,
            "session.assistant_failed",
            json!({"sid":voice.session_id,"reason":"telephone audio did not become ready"}),
        )?;
        let _ = terminate(state, &SessionId::from_string(&voice.session_id)).await;
        return Err(ApiError::internal(format!(
            "telephone media readiness: {error}"
        )));
    }
    let role_instructions = match target.role.as_str() {
        "booker" => "State the traveler's known requested changes and ask what the reservationist can offer; the reservationist supplies flight availability and times. Read the offered details back and finish the confirmation. When the reservationist says goodbye or clearly ends the conversation, give one short goodbye and stop speaking. Do not ask further questions or restart the conversation after their goodbye. Invoke finish_call with reason completed once your readback is finished, or participant_goodbye when they end early. The tool supplies your one closing goodbye; do not say another goodbye yourself. Respect their goodbye even when some itinerary facts are missing; never invent those facts.",
        "organizer" => "Give the confirmed itinerary from your call task and ask the organizer to confirm pickup and the meeting point. The organizer arranges ground logistics. Once pickup is confirmed, acknowledge it briefly and invoke request_browser_join exactly once. The tool announces that you are bringing Jonathan into this same call and asks the external coordinator to ring his browser. Merely saying you will bring him in does not invoke the tool. After invoking it, stop speaking and wait quietly; keep the call open. If the organizer has a question only Jonathan can answer, invite him to answer it. Do not speak on Jonathan's behalf, address him as if he has already joined, or claim the handoff has happened. The actual UCTP browser handoff is performed by the external coordinator and Jonathan.",
        _ => "Follow the task for this participant and acknowledge the facts they provide.",
    };
    let finish_instructions = "If an answering AI, receptionist or voicemail cannot perform this task, ask once whether they can connect you to the intended person. If they cannot, decline further assistance and invoke finish_call with reason unavailable immediately. Do not repeat the task, negotiate with an incapable assistant, trade repeated goodbyes, or claim the task was completed. When someone explicitly ends the conversation, invoke finish_call with reason participant_goodbye. The tool speaks one brief goodbye; after invoking it remain silent, including if the other assistant responds. Never invoke finish_call during or after a successful Jonathan browser/phone handoff; he controls that call.";
    let prompt=format!("You are {}, Jonathan's AI travel coordination assistant. This is a conference demonstration; travel booking is sandbox only. You are the caller, speaking with {}. Your task for this call: {}. {} {} Record the facts they provide. Never claim a booking, SMS, human approval, or other external action occurred unless the supplied context says so. Use only the explicitly supplied tools; do not make independent phone calls. Keep this conversation concise and friendly. Keep the call open when the task says Jonathan will join. The external coordinator controls other communications and call termination through UCTP.",assistant.name,target.name,voice.purpose,role_instructions,finish_instructions);
    let mut tools = if target.role == "organizer" {
        json!([{
            "type":"function", "async":true,
            "function":{
                "name":"request_browser_join",
                "description":"After the organizer confirms pickup, invite Jonathan's browser into this existing call. Invoke once, then wait quietly. The coordinator resolves all identities and routes; this tool takes no arguments.",
                "parameters":{"type":"object","properties":{},"additionalProperties":false}
            },
            "messages":[{"type":"request-start","content":"Let me bring Jonathan into this same call so he can confirm.","blocking":true}]
        }])
    } else {
        json!([])
    };
    tools.as_array_mut().expect("voice tools array").push(json!({
        "type":"function", "async":true,
        "function":{
            "name":"finish_call",
            "description":"End this call after one brief goodbye when the task is completed, the answering party cannot help or transfer you, or they explicitly say goodbye. Invoke once, then stay silent. Never use after a human handoff. The coordinator ends only this Session.",
            "parameters":{"type":"object","properties":{"reason":{"type":"string","enum":["completed","unavailable","participant_goodbye"]}},"required":["reason"],"additionalProperties":false}
        },
        "messages":[{"type":"request-start","content":"Thank you for your time. Goodbye.","blocking":true}]
    }));
    let options=VapiCallOptions::new(VapiAssistant::saved_with_overrides(state.config.vapi_assistant_id.clone(),json!({
        "model":{"provider":"openai","model":"gpt-4o-mini","messages":[{"role":"system","content":prompt}],"tools":tools},
        "firstMessage":format!("Hello {}, I'm {}, Jonathan's AI assistant helping coordinate his conference travel. Is now a good time?",target.name,assistant.name),
        // The organizer waits while the coordinator invites the browser. Keep
        // that normal pause alive; the owner still explicitly ends the Session.
        "silenceTimeoutSeconds":if target.role == "organizer" { 180 } else { 30 },
        "maxDurationSeconds":600,
        "serverMessages":[],"clientMessages":["transcript","status-update","speech-update","tool-calls"]
    }))).with_audio_format(VapiAudioFormat::PcmS16Le16Khz).with_name("Parley conference")
      .with_metadata(json!({"conversation_id":voice.conversation_id,"session_id":voice.session_id,"participant_id":voice.assistant_participant_id}));
    let call = match adapter
        .attach_agent_for_participant(
            &state.orchestrator,
            rvoip_core::ids::ConnectionId::from_string(&voice.remote_connection_id),
            ParticipantId::from_string(&voice.assistant_participant_id),
            options,
        )
        .await
    {
        Ok(call) => call,
        Err(error) => {
            let reason = match &error {
                rvoip_core::RvoipError::InvalidState(reason) => *reason,
                rvoip_core::RvoipError::Adapter(reason)
                    if matches!(
                        reason.as_str(),
                        "Vapi HTTP request failed"
                            | "Vapi HTTP request timed out"
                            | "Vapi WebSocket setup failed"
                            | "Vapi WebSocket operation timed out"
                    ) =>
                {
                    reason.as_str()
                }
                _ => error.diagnostic_class(),
            };
            tracing::warn!(reason, session_id=%voice.session_id, "conference Vapi attachment failed");
            state.store.conference_voice_fact(
                tenant,
                &voice,
                "session.assistant_failed",
                json!({"sid":voice.session_id,"reason":"Vapi attachment failed"}),
            )?;
            let _ = terminate(state, &SessionId::from_string(&voice.session_id)).await;
            return Err(core_error(error));
        }
    };
    state.store.conference_ai_attached(
        tenant,
        &voice,
        &call.vapi_connection_id().to_string(),
        &call.bridge_id().to_string(),
    )?;
    state.store.conference_voice_fact(
        tenant, &voice, "session.assistant_actions",
        json!({"sid":voice.session_id,"participant_id":voice.assistant_participant_id,
            "actions":if target.role == "organizer" { vec!["request_browser_join", "finish_call"] } else { vec!["finish_call"] },"source":"vapi"}),
    )?;
    state
        .vapi_calls
        .lock()
        .expect("vapi calls")
        .insert(voice.session_id.clone(), call.vapi_connection_id().clone());
    let mut events = call.subscribe_events();
    let state = state.clone();
    let organizer_tool = target.role == "organizer";
    tokio::spawn(async move {
        let completed = call.wait_shared();
        tokio::pin!(completed);
        loop {
            tokio::select! {
                _=&mut completed=>break,
                event=events.recv()=>match event {
                    Ok(VapiEvent::SpeechUpdate{status,role,turn}) if matches!(status.as_str(),"started"|"stopped")=>{
                        let speaker=match role.as_deref() {Some("user")=>Some(&voice.target_participant_id),Some("assistant")=>Some(&voice.assistant_participant_id),_=>None};
                        if let Some(speaker)=speaker {
                            let _=state.store.conference_voice_fact(&state.config.tenant_id,&voice,"session.speech",json!({"sid":voice.session_id,"speaker":speaker,"state":status,"turn":turn,"source":"vapi"}));
                        }
                    }
                    Ok(VapiEvent::Transcript{role,transcript_type,transcript}) if transcript_type.as_deref()==Some("final")=>{
                        let speaker=match role.as_deref() {Some("user")=>Some(&voice.target_participant_id),Some("assistant")=>Some(&voice.assistant_participant_id),_=>None};
                        if let (Some(speaker),Some(text))=(speaker,transcript) {
                            let _=state.store.conference_voice_fact(&state.config.tenant_id,&voice,"session.transcript",json!({"sid":voice.session_id,"speaker":speaker,"text":text,"is_final":true,"source":"vapi"}));
                        }
                    }
                    Ok(VapiEvent::ToolCall{event_type,payload}) if event_type=="tool-calls"=>{
                        for tool_call_id in browser_join_tool_calls(&payload).into_iter().filter(|_| organizer_tool) {
                            let _=state.store.conference_voice_fact(&state.config.tenant_id,&voice,"session.assistant_action",
                                json!({"sid":voice.session_id,"participant_id":voice.assistant_participant_id,
                                    "action":"request_browser_join","tool_call_id":tool_call_id,"source":"vapi"}));
                        }
                        for (tool_call_id, reason) in finish_call_tool_calls(&payload) {
                            let _=state.store.conference_voice_fact(&state.config.tenant_id,&voice,"session.assistant_action",
                                json!({"sid":voice.session_id,"participant_id":voice.assistant_participant_id,
                                    "action":"finish_call","reason":reason,"tool_call_id":tool_call_id,"source":"vapi"}));
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed)=>break,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_))=>{
                        let _=state.store.conference_voice_fact(&state.config.tenant_id,&voice,"session.transcript_gap",json!({"sid":voice.session_id,"reason":"provider event lag"}));
                    }
                    _=>{}
                }
            }
        }
        state
            .vapi_calls
            .lock()
            .expect("vapi calls")
            .remove(&voice.session_id);
    });
    Ok(())
}

/// Decode only the zero-argument tool attached to this provider connection.
/// Model-supplied routing IDs, arbitrary tool names and malformed calls are rejected.
fn browser_join_tool_calls(payload: &Value) -> Vec<&str> {
    payload
        .get("toolCallList")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|call| {
            let id = call.get("id")?.as_str()?;
            if id.is_empty()
                || id.len() > 128
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            {
                return None;
            }
            let function = call.get("function")?;
            if function.get("name")?.as_str()? != "request_browser_join" {
                return None;
            }
            let arguments = function.get("arguments")?;
            let parsed = if let Some(text) = arguments.as_str() {
                serde_json::from_str::<Value>(text).ok()?
            } else {
                arguments.clone()
            };
            parsed
                .as_object()
                .filter(|args| args.is_empty())
                .map(|_| id)
        })
        .collect()
}

/// A provider tool may finish only its attached Session, with a bounded reason.
fn finish_call_tool_calls(payload: &Value) -> Vec<(&str, String)> {
    payload
        .get("toolCallList")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|call| {
            let id = call.get("id")?.as_str()?;
            if id.is_empty()
                || id.len() > 128
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            {
                return None;
            }
            let function = call.get("function")?;
            if function.get("name")?.as_str()? != "finish_call" {
                return None;
            }
            let arguments = function.get("arguments")?;
            let parsed = if let Some(text) = arguments.as_str() {
                serde_json::from_str::<Value>(text).ok()?
            } else {
                arguments.clone()
            };
            let args = parsed.as_object()?;
            let reason = args.get("reason")?.as_str()?;
            if args.len() != 1
                || !matches!(reason, "completed" | "unavailable" | "participant_goodbye")
            {
                return None;
            }
            Some((id, reason.to_owned()))
        })
        .collect()
}

#[cfg(test)]
mod voice_tool_tests {
    use super::*;
    #[test]
    fn finish_call_accepts_only_bounded_reasons_without_routing_arguments() {
        let call = |args: Value| json!({"id":"call_finish-123","function":{"name":"finish_call","arguments":args}});
        for reason in ["completed", "unavailable", "participant_goodbye"] {
            for args in [
                json!({"reason":reason}),
                json!(format!(r#"{{"reason":"{reason}"}}"#)),
            ] {
                assert_eq!(
                    finish_call_tool_calls(&json!({"toolCallList":[call(args)]})),
                    vec![("call_finish-123", reason.to_owned())]
                );
            }
        }
        for args in [
            json!({}),
            json!({"reason":"unavailable","sid":"another-session"}),
            json!({"reason":"anything"}),
            json!(null),
            json!("{"),
            json!([]),
        ] {
            assert!(finish_call_tool_calls(&json!({"toolCallList":[call(args)]})).is_empty());
        }
        assert!(finish_call_tool_calls(&json!({"toolCallList":[{"id":"bad id","function":{"name":"finish_call","arguments":{"reason":"unavailable"}}}]})).is_empty());
    }

    #[test]
    fn browser_join_accepts_only_valid_zero_argument_provider_calls() {
        let call = |name: &str, args: Value| json!({"id":"call_abc-123","function":{"name":name,"arguments":args}});
        assert_eq!(
            browser_join_tool_calls(
                &json!({"toolCallList":[call("request_browser_join",json!("{}")),call("request_browser_join",json!({}))]})
            ),
            vec!["call_abc-123", "call_abc-123"]
        );
        for value in [
            json!(null),
            json!("{"),
            json!({"sid":"other-call"}),
            json!([]),
        ] {
            assert!(browser_join_tool_calls(
                &json!({"toolCallList":[call("request_browser_join",value)]})
            )
            .is_empty());
        }
        assert!(
            browser_join_tool_calls(&json!({"toolCallList":[call("end_voice",json!({}))]}))
                .is_empty()
        );
    }
}
