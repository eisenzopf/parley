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
        let mut config = state
            .config
            .conference_network
            .sip_config(addr, &state.config.conference_sip_from)?;
        if !state.config.conference_sip_caller_pool.is_empty() {
            // With a pool, each call's From supplies the owned caller ID.
            // A fixed P-Asserted-Identity would override that number at Telnyx.
            config.pai_uri = None;
        }
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
        let caller = crate::conference_network::caller_identity(state, tenant, cid)?;
        let ticket = state.orchestrator.prepare_outbound_connection(state.config.conference_network.sip_request(rvoip_core::OriginateRequest::new(
            sid.clone(),ParticipantId::from_string(&target.participant_id),endpoint,rvoip_core::Direction::Outbound,Default::default()
        ).with_transport(Transport::Sip), &caller)?).await.map_err(core_error)?;
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

/// Owner starts the demo by calling David over WebRTC, without dialing a phone.
#[cfg(feature = "media-webrtc")]
pub async fn call_david(
    state: &AppState,
    tenant: &str,
    cid: &str,
    owner: &Member,
    purpose: &str,
    request: &str,
) -> Result<Value> {
    if owner.role != "owner" {
        return Err(ApiError::forbidden("only the owner can call David"));
    }
    if purpose.trim().is_empty() || purpose.len() > 4000 {
        return Err(ApiError::bad_request(
            "travel context required, maximum 4000 bytes",
        ));
    }
    if !assistant_available(state) {
        return Err(ApiError::conflict("David's voice is unavailable"));
    }
    let _guard = state.conference_handoff.lock().await;
    if state.store.count_live_voice_sessions(tenant, cid)? > 0 {
        return Err(ApiError::conflict("a voice call is already active"));
    }
    let assistant = state
        .store
        .conference_members(tenant, cid)?
        .into_iter()
        .find(|m| m.role == "assistant")
        .ok_or_else(|| ApiError::conflict("assistant missing"))?;
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
    let result=async {
        for (m,kind,role) in [(owner,ParticipantKind::Human,ParticipantRole::Customer),(&assistant,ParticipantKind::Ai,ParticipantRole::Agent)] {
            state.orchestrator.join_session(sid.clone(),ParticipantId::from_string(&m.participant_id),kind,role).await.map_err(core_error)?;
        }
        let ticket=state.orchestrator.prepare_outbound_connection(rvoip_core::OriginateRequest::new(sid.clone(),ParticipantId::from_string(&owner.participant_id),"uctp-browser",rvoip_core::Direction::Outbound,rvoip_core::adapter::ConnectionAdapter::capabilities(state.conference_browser.as_ref())).with_transport(Transport::WebRtc)).await.map_err(core_error)?;
        let connid=ticket.connection_id().to_string();
        state.store.prepare_conference_voice_transport(tenant,cid,&sid.to_string(),request,owner,&assistant,&connid,purpose,&owner.participant_id,"webrtc")?;
        let voice=state.store.conference_voice_for_session(tenant,&sid.to_string())?.ok_or_else(||ApiError::internal("intro call missing"))?;
        state.store.prepare_conference_browser(tenant,&voice,&owner.participant_id,&connid,request)?;
        ticket.commit().await.map_err(core_error)?;
        let sdp=state.conference_browser.local_sdp(&rvoip_core::ids::ConnectionId::from_string(&connid)).map_err(|_|ApiError::internal("browser offer unavailable"))?;
        let cleanup=state.clone();let cleanup_sid=sid.clone();let cleanup_tenant=tenant.to_string();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            if cleanup.store.conference_voice_for_session(&cleanup_tenant,&cleanup_sid.to_string()).ok().flatten().is_some_and(|v|v.ai_state=="waiting") {
                let _=terminate(&cleanup,&cleanup_sid).await;
            }
        });
        Ok(json!({"sid":sid.to_string(),"connid":connid,"substrate":"webrtc","substrate_setup":{"sdp_type":"offer","sdp":sdp},"ice_servers":state.config.conference_network.browser_ice}))
    }.await;
    if result.is_err() {
        let _ = terminate(state, &sid).await;
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
        if let Some(mut voice) = state.store.conference_voice_for_session(tenant, sid)? {
            voice.request_id = request.to_string();
            state.store.conference_voice_fact(
                tenant,
                &voice,
                "session.ending",
                json!({"sid":sid,"initiated_by":actor.participant_id}),
            )?;
        }
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
        // A dropped private browser must not leave Jeff stranded on hold.
        let consult_sid = state
            .store
            .consultation_for_browser(tenant, &connection_id.to_string())?;
        if let Some(sid) = consult_sid {
            let state = state.clone();
            tokio::spawn(async move {
                let _handoff = state.conference_handoff.lock().await;
                let _ = terminate(&state, &SessionId::from_string(sid)).await;
            });
        }
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
    if !matches!(voice.ai_state.as_str(), "attached" | "held")
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

pub(crate) fn core_error(error: rvoip_core::RvoipError) -> ApiError {
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
    #[cfg(all(feature = "media-webrtc", feature = "vapi"))]
    let _ = crate::conference_consult::stop_hold(state, &sid.to_string()).await;
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
    use rvoip_vapi::{VapiAssistant, VapiAudioFormat, VapiCallOptions};
    let tenant = &state.config.tenant_id;
    if !matches!(
        state.config.conference_voice_model.as_str(),
        "gpt-realtime-2" | "gpt-4o-mini"
    ) {
        return Err(ApiError::conflict("unsupported conference voice model"));
    }
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
        "owner" => "# Opening call from Jonathan\nJonathan has called you in his browser. Your first message is 'Jonathan, are you on your flight?' Wait for his answer. If he says he missed his flight or needs a replacement, invoke finish_call with reason completed immediately. The coordinator will say 'Again? Okay, let me see what I can do. I’ll call you back.' with a playful, friendly tone and then start calling the travel agent. Do not say that line yourself or ask for extra confirmations, flight numbers, gates, or itinerary details. The supplied background has the travel context. If he says he is on his flight, respond naturally and do not start travel coordination. If he wants to end without help, use finish_call with reason participant_goodbye. Never claim you have already booked anything.",
        "booker" => "# Booking call\n- You are speaking ONLY with the travel agent for this entire call. Never address the organizer or Jonathan, start another greeting, or narrate later coordination here.\n- Help Jonathan and Andrew find a replacement for their missed flight. Give the known route and requested times as preferences, not confirmed inventory.\n- ONLY collect the offered airline, flight number, departure time, and arrival time. These four facts complete this call.\n- Do not ask for gates, terminals, seat counts, booking references, or other flight details. Traveler count is context, not another verification question.\n- Let the booking agent finish. Accept clear volunteered facts the first time; ask one short question only for a missing or genuinely unclear required fact.\n- Briefly acknowledge useful new information in your own words. Do not acknowledge every sentence or repeat the same phrase.\n- Example when only arrival is missing: 'Nice, and when does that get them in?' Example when only the airline is missing: 'Who's that with?' These are examples, not a script.\n- NO final readback or 'is that correct?' loop. Once all four facts are clear, invoke finish_booking with the airline, flight_number, departure_time, and arrival_time exactly as supplied. Never call it while a fact is missing; ask for only that missing fact and WAIT for the answer. Asking a question does not supply its answer. The coordinator says the casual thank-you and goodbye; do not add your own closing.\n- If the booking agent says goodbye, invoke finish_call with reason participant_goodbye even if facts are missing. Never invent missing facts.",
        "organizer" => "# Organizer call\n- Give a friendly heads-up: Jonathan missed his flight and is coming in a little late. Use the booking agent's actual arrival time. Example: 'Hey, quick heads-up: Jonathan missed his flight, so he'll be a little late. He's getting in at four.' Adapt to the actual facts.\n- Do NOT ask for pickup, a ride, a terminal, or a meeting point, even if old call context mentions pickup.\n- Let the organizer respond. Be ready for them to ask whether Jonathan can cut his presentation thirty minutes short because he is late. Do not suggest that request or invent it.\n- If they ask to shorten the presentation, react with a small natural laugh and bring Jonathan in so they can ask him directly. NEVER agree to change his presentation or answer for him.\n- Invoke request_browser_join exactly once when the organizer asks that question, asks to speak with Jonathan, or raises another decision that needs him. A greeting alone is not a handoff request.\n- The tool's coordinator speaks 'Ha! Hold on, let me get Jonathan on the line so you can tell him that.', waits for it to finish, then rings his browser. Deliver the line warmly, with a light chuckle, not exaggerated laughter. Do not give a separate handoff announcement or laugh before invoking the tool.\n- After invoking the tool, stop speaking and keep this same call open. When Jonathan answers, he first speaks privately with you while Jeff waits. Ask permission to connect Jeff; invoke connect_organizer only after Jonathan clearly agrees.",
        _ => "Follow the task for this participant and acknowledge the facts they provide.",
    };
    let finish_instructions = "If an answering AI, receptionist or voicemail cannot do the task, ask once whether they can connect you to the intended person. If they cannot, invoke finish_call with reason unavailable immediately. When someone explicitly ends the conversation, invoke finish_call with reason participant_goodbye. The coordinator speaks the goodbye; remain silent after invoking the tool. Never invoke finish_call during or after Jonathan's browser/phone handoff.";
    let prompt=format!("# Role & Objective\nYou are {}, Jonathan's AI assistant calling {} to help with his travel. This is a sandbox travel demonstration.\n# Personality & Tone\n- Casual, informal, warm, and a little playful: Jonathan's helpful assistant who is easy to talk to.\n- Use contractions and everyday English. Keep replies short and speak at a relaxed, conversational pace.\n- Let light humor come naturally if the other person is playful. Do not force jokes, banter, or extra turns.\n- Vary your phrasing. Avoid repetitive 'perfect', 'absolutely', formal service language, and narrating your checklist.\n# Call context (background, not a script; the role-specific call instructions below define what to collect)\n{}\n# Conversation\n- Usually say one short sentence or question, then listen. Let the other person talk more than you do.\n- Ask only about the next missing fact. Do not repeat a question, recap, or confirmation already answered.\n- If interrupted, stop and listen. The opening message already introduced you.\n{}\n# Tools and facts\n{}\n- Record only facts the other person supplies. Never claim a booking, SMS, or approval occurred unless the context says so.\n- Use only the supplied tools; the external UCTP coordinator controls other calls and termination.\n- The current participant stays the same until this call ends. Only an organizer call stays open for a human handoff; a booking call must close after its goodbye.",assistant.name,target.name,voice.purpose,role_instructions,finish_instructions);
    let mut tools = if target.role == "organizer" {
        json!([{
            "type":"function", "async":true,
            "function":{
                "name":"request_browser_join",
                "description":"When the organizer asks to shorten Jonathan's presentation, asks to speak with him, or raises a decision for him, invite Jonathan's browser into this existing call. Invoke once; the coordinator speaks the casual handoff line, then rings him. Stay silent and keep the call open. This tool takes no arguments.",
                "parameters":{"type":"object","properties":{},"additionalProperties":false}
            },
            "messages":[{"type":"request-start","content":""}]
        }])
    } else {
        json!([])
    };
    tools.as_array_mut().expect("voice tools array").push(json!({
        "type":"function", "async":true,
        "function":{
            "name":"finish_call",
            "description":"End this call when the task is completed, the answering party cannot help or transfer you, or they explicitly say goodbye. Invoke once; the coordinator speaks the thank-you and goodbye, so stay silent. Never use after a human handoff. The coordinator ends only this Session.",
            "parameters":{"type":"object","properties":{"reason":{"type":"string","enum":if target.role == "booker" { json!(["unavailable","participant_goodbye"]) } else { json!(["completed","unavailable","participant_goodbye"]) }}},"required":["reason"],"additionalProperties":false}
        },
        // Client-side tool messages are not reliable on the raw WebSocket
        // transport. The attributed tool event below queues the exact audio.
        "messages":[{"type":"request-start","content":""}]
    }));
    // Saved-assistant overrides merge tools instead of replacing them. Use a
    // transient definition so legacy webhook tools and prompts cannot leak
    // into this coordinator-owned call.
    if target.role == "booker" {
        tools.as_array_mut().expect("voice tools array").push(json!({
            "type":"function", "async":true,
            "function":{
                "name":"finish_booking",
                "description":"End the travel agent call only after the agent has supplied all four flight facts. Copy the offered values, never preferences or invented answers. If a fact is missing, ask for it and wait. The coordinator says goodbye and hangs up; stay silent after invoking this tool.",
                "parameters":{"type":"object","properties":{
                    "airline":{"type":"string","minLength":1},
                    "flight_number":{"type":"string","minLength":1},
                    "departure_time":{"type":"string","minLength":1},
                    "arrival_time":{"type":"string","minLength":1}
                },"required":["airline","flight_number","departure_time","arrival_time"],"additionalProperties":false}
            },"messages":[{"type":"request-start","content":""}]
        }));
    }
    let mut definition = json!({
        "name":"Parley conference David",
        "model":{"provider":"openai","model":state.config.conference_voice_model,"messages":[{"role":"system","content":prompt}],"tools":tools},
        "firstMessage":format!("Hi, this is {} calling for Jonathan. Is now a good time?",assistant.name),
        "endCallPhrases":[],
        "endCallFunctionEnabled":false,
        // The organizer waits while the coordinator invites the browser. Keep
        // that normal pause alive; the owner still explicitly ends the Session.
        "silenceTimeoutSeconds":if target.role == "organizer" { 180 } else { 30 },
        "maxDurationSeconds":600,
        "serverMessages":[],"clientMessages":["transcript","status-update","speech-update","tool-calls"]
    });
    if target.role == "owner" {
        definition["firstMessage"] = json!("Jonathan, are you on your flight?");
        definition["silenceTimeoutSeconds"] = json!(60);
    }
    match state.config.conference_voice_model.as_str() {
        "gpt-realtime-2" => {
            // Realtime handles audio natively; Vapi converts the system message
            // into session instructions. Complete tools through our webhook:
            // Vapi's async client-only result can arrive after Realtime has
            // dropped its call ID, ending the call with invalid_tool_call_id.
            definition["model"]["maxTokens"] = json!(250);
            definition["voice"] = json!({"provider":"openai","voiceId":"cedar"});
            let server = json!({
                "url":format!("{}/v1/vapi/conference-tools/{}/{}",
                    state.config.public_http_base().trim_end_matches('/'),
                    voice.session_id,
                    crate::vapi_tools::conference_tool_signature(&state.config.vapi_api_key,&voice.session_id))
            });
            for tool in definition["model"]["tools"]
                .as_array_mut()
                .expect("voice tools array")
            {
                tool["async"] = json!(false);
                tool["server"] = server.clone();
            }
        }
        "gpt-4o-mini" => {} // Previous conference voice path for rollback.
        _ => unreachable!("conference voice model checked before claim"),
    }
    let options=VapiCallOptions::new(VapiAssistant::transient(definition))
      .with_audio_format(VapiAudioFormat::PcmS16Le16Khz).with_name("Parley conference")
      .with_peer_failure_policy(rvoip_vapi::VapiPeerFailurePolicy::LeaveCallerConnected)
      .with_metadata(json!({"conversation_id":voice.conversation_id,"session_id":voice.session_id,"participant_id":voice.assistant_participant_id,"voice_profile":"conference-voice/3"}));
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
    observe_call(state, voice, call, target.role == "organizer");
    Ok(())
}

#[cfg(feature = "vapi")]
pub(crate) fn observe_call(
    state: &AppState,
    voice: crate::store::conference::VoiceOperation,
    call: rvoip_vapi::VapiAgentCall,
    organizer_tool: bool,
) {
    use rvoip_vapi::VapiEvent;
    let adapter = state.vapi_adapter.as_ref().expect("voice adapter").clone();
    state
        .vapi_calls
        .lock()
        .expect("vapi calls")
        .insert(voice.session_id.clone(), call.vapi_connection_id().clone());
    let mut events = call.subscribe_events();
    let intro = state
        .store
        .conference_members(&state.config.tenant_id, &voice.conversation_id)
        .ok()
        .is_some_and(|members| {
            members
                .iter()
                .any(|m| m.role == "owner" && m.participant_id == voice.target_participant_id)
        });
    let booker = state
        .store
        .conference_members(&state.config.tenant_id, &voice.conversation_id)
        .ok()
        .is_some_and(|members| {
            members
                .iter()
                .any(|m| m.role == "booker" && m.participant_id == voice.target_participant_id)
        });
    let state = state.clone();
    tokio::spawn(async move {
        let mut announced_join = false;
        let mut announced_finish = false;
        let mut requested_transfer = false;
        let completed = call.wait_shared();
        tokio::pin!(completed);
        loop {
            tokio::select! {
                _=&mut completed=>{
                    // A consult changes the AI's peer. Parley owns cleanup while
                    // the old paired supervisor leaves the retained phone alive.
                    let _guard=state.conference_handoff.lock().await;
                    if let Ok(Some(current))=state.store.conference_voice_for_session(&state.config.tenant_id,&voice.session_id) {
                        if current.ai_connection_id.as_deref()==Some(call.vapi_connection_id().to_string().as_str()) && matches!(current.ai_state.as_str(),"attached"|"consulting") {
                            let _=state.store.record_conference_ai_failure(&state.config.tenant_id,&call.vapi_connection_id().to_string());
                            let _=terminate(&state,&SessionId::from_string(&voice.session_id)).await;
                        } else if current.ai_connection_id.as_deref()==Some(call.vapi_connection_id().to_string().as_str()) && current.ai_state=="ending" && announced_finish && booker {
                            // endCallAfterSpoken closed the AI after its goodbye.
                            // Close the retained SIP leg as a deliberate ending.
                            let _=terminate(&state,&SessionId::from_string(&voice.session_id)).await;
                        } else if current.ai_connection_id.as_deref()==Some(call.vapi_connection_id().to_string().as_str()) && current.ai_state=="ending" && announced_join && organizer_tool {
                            if crate::conference_consult::hold_organizer(&state,&state.config.tenant_id,&current).await.is_err() {
                                let _=terminate(&state,&SessionId::from_string(&voice.session_id)).await;
                            }
                        }
                    }
                    break;
                },
                event=events.recv()=>match event {
                    Ok(VapiEvent::SpeechUpdate{status,role,turn}) if matches!(status.as_str(),"started"|"stopped")=>{
                        let consult=state.store.browser_consult(&state.config.tenant_id,&voice.session_id).ok().flatten();
                        let remote_speaker=consult.as_ref().map(|r|r.participant_id.as_str()).unwrap_or(&voice.target_participant_id);
                        let speaker=match role.as_deref() {Some("user")=>Some(remote_speaker),Some("assistant")=>Some(voice.assistant_participant_id.as_str()),_=>None};
                        if let Some(speaker)=speaker {
                            let _=state.store.conference_voice_fact(&state.config.tenant_id,&voice,"session.speech",json!({"sid":voice.session_id,"speaker":speaker,"state":status,"turn":turn,"source":"vapi"}));
                        }
                    }
                    Ok(VapiEvent::Transcript{role,transcript_type,transcript}) if transcript_type.as_deref()==Some("final")=>{
                        let consult=state.store.browser_consult(&state.config.tenant_id,&voice.session_id).ok().flatten();
                        let remote_speaker=consult.as_ref().map(|r|r.participant_id.as_str()).unwrap_or(&voice.target_participant_id);
                        let speaker=match role.as_deref() {Some("user")=>Some(remote_speaker),Some("assistant")=>Some(voice.assistant_participant_id.as_str()),_=>None};
                        if let (Some(speaker),Some(text))=(speaker,transcript) {
                            let _=state.store.conference_voice_fact(&state.config.tenant_id,&voice,"session.transcript",json!({"sid":voice.session_id,"speaker":speaker,"text":text,"is_final":true,"source":"vapi"}));
                        }
                    }
                    Ok(VapiEvent::ToolCall{event_type,payload}) if event_type=="tool-calls"=>{
                        for tool_call_id in browser_join_tool_calls(&payload).into_iter().filter(|_| organizer_tool) {
                            if announced_join || announced_finish { continue; }
                            announced_join = true;
                            let announcement_queued = adapter.say(call.vapi_connection_id(),
                                "Ha! Hold on, let me get Jonathan on the line so you can tell him that.", true, false).await.is_ok();
                            if announcement_queued {
                                let _=state.store.mark_conference_ai_ending(&state.config.tenant_id,&voice.session_id);
                            }
                            let _=state.store.conference_voice_fact(&state.config.tenant_id,&voice,"session.assistant_action",
                                json!({"sid":voice.session_id,"participant_id":voice.assistant_participant_id,
                                    "action":"request_browser_join","tool_call_id":tool_call_id,"announcement_queued":announcement_queued,"hold_before_invitation":true,"source":"vapi"}));
                        }
                        for tool_call_id in connect_organizer_tool_calls(&payload).into_iter().filter(|_| organizer_tool) {
                            if requested_transfer || state.store.browser_consult(&state.config.tenant_id,&voice.session_id).ok().flatten().is_none() { continue; }
                            requested_transfer = true;
                            let _=state.store.conference_voice_fact(&state.config.tenant_id,&voice,"session.assistant_action",
                                json!({"sid":voice.session_id,"participant_id":voice.assistant_participant_id,
                                    "action":"connect_organizer","tool_call_id":tool_call_id,"source":"vapi"}));
                        }
                        for (tool_call_id, reason) in finish_call_tool_calls(&payload, booker) {
                            if state.store.browser_consult(&state.config.tenant_id,&voice.session_id).ok().flatten().is_some() { continue; }
                            if announced_finish { continue; }
                            announced_finish = true;
                            let announcement_queued = adapter.say(call.vapi_connection_id(),
                                if intro && reason=="completed" { "Again? Okay, let me see what I can do. I'll call you back." } else if intro { "Okay, talk soon!" } else { "Thanks for your help! Bye." }, booker, false).await.is_ok();
                            if announcement_queued && booker {
                                let _=state.store.mark_conference_ai_ending(&state.config.tenant_id,&voice.session_id);
                            }
                            let _=state.store.conference_voice_fact(&state.config.tenant_id,&voice,"session.assistant_action",
                                json!({"sid":voice.session_id,"participant_id":voice.assistant_participant_id,
                                    "action":"finish_call","reason":reason,"tool_call_id":tool_call_id,"announcement_queued":announcement_queued,"source":"vapi"}));
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
        let mut calls = state.vapi_calls.lock().expect("vapi calls");
        if calls.get(&voice.session_id) == Some(call.vapi_connection_id()) {
            calls.remove(&voice.session_id);
        }
    });
}

/// Decode only the zero-argument tool attached to this provider connection.
/// Model-supplied routing IDs, arbitrary tool names and malformed calls are rejected.
pub(crate) fn browser_join_tool_calls(payload: &Value) -> Vec<&str> {
    zero_argument_tool_calls(payload, "request_browser_join")
}

pub(crate) fn connect_organizer_tool_calls(payload: &Value) -> Vec<&str> {
    zero_argument_tool_calls(payload, "connect_organizer")
}

fn zero_argument_tool_calls<'a>(payload: &'a Value, name: &str) -> Vec<&'a str> {
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
            if function.get("name")?.as_str()? != name {
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
pub(crate) fn finish_call_tool_calls(payload: &Value, booker: bool) -> Vec<(&str, String)> {
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
            let name = function.get("name")?.as_str()?;
            if name != "finish_call" && !(booker && name == "finish_booking") {
                return None;
            }
            let arguments = function.get("arguments")?;
            let parsed = if let Some(text) = arguments.as_str() {
                serde_json::from_str::<Value>(text).ok()?
            } else {
                arguments.clone()
            };
            let args = parsed.as_object()?;
            if name == "finish_booking" {
                let fields = ["airline", "flight_number", "departure_time", "arrival_time"];
                if args.len() != fields.len()
                    || !fields.iter().all(|key| {
                        args.get(*key).and_then(Value::as_str).is_some_and(|value| {
                            let value = value.trim();
                            !value.is_empty()
                                && value.len() <= 256
                                && !matches!(
                                    value.to_lowercase().as_str(),
                                    "unknown" | "not provided" | "n/a" | "tbd" | "pending"
                                )
                        })
                    })
                {
                    return None;
                }
                return Some((id, "completed".to_string()));
            }
            let reason = args.get("reason")?.as_str()?;
            if args.len() != 1
                || (booker && reason == "completed")
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
    fn booking_finish_requires_all_four_facts_and_cannot_use_generic_completion() {
        let payload = |name: &str, args: Value| json!({"toolCallList":[{"id":"booking-1","function":{"name":name,"arguments":args}}]});
        let facts = json!({"airline":"American Airlines","flight_number":"AA 123","departure_time":"11 AM","arrival_time":"7 PM"});
        assert_eq!(
            finish_call_tool_calls(&payload("finish_booking", facts.clone()), true),
            vec![("booking-1", "completed".to_string())]
        );
        assert!(
            finish_call_tool_calls(&payload("finish_booking", facts.clone()), false).is_empty()
        );
        assert!(finish_call_tool_calls(
            &payload("finish_call", json!({"reason":"completed"})),
            true
        )
        .is_empty());
        for missing in ["airline", "flight_number", "departure_time", "arrival_time"] {
            let mut incomplete = facts.clone();
            incomplete.as_object_mut().unwrap().remove(missing);
            assert!(
                finish_call_tool_calls(&payload("finish_booking", incomplete), true).is_empty()
            );
        }
        for invalid in [json!(""), json!("unknown"), json!(null)] {
            let mut incomplete = facts.clone();
            incomplete["flight_number"] = invalid;
            assert!(
                finish_call_tool_calls(&payload("finish_booking", incomplete), true).is_empty()
            );
        }
        assert_eq!(
            finish_call_tool_calls(
                &payload("finish_call", json!({"reason":"participant_goodbye"})),
                true
            ),
            vec![("booking-1", "participant_goodbye".to_string())]
        );
    }
    #[test]
    fn finish_call_accepts_only_bounded_reasons_without_routing_arguments() {
        let call = |args: Value| json!({"id":"call_finish-123","function":{"name":"finish_call","arguments":args}});
        for reason in ["completed", "unavailable", "participant_goodbye"] {
            for args in [
                json!({"reason":reason}),
                json!(format!(r#"{{"reason":"{reason}"}}"#)),
            ] {
                assert_eq!(
                    finish_call_tool_calls(&json!({"toolCallList":[call(args)]}), false),
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
            assert!(
                finish_call_tool_calls(&json!({"toolCallList":[call(args)]}), false).is_empty()
            );
        }
        assert!(finish_call_tool_calls(&json!({"toolCallList":[{"id":"bad id","function":{"name":"finish_call","arguments":{"reason":"unavailable"}}}]}), false).is_empty());
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
