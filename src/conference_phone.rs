//! Owner-initiated browser → telephone move inside the existing voice Session.
use crate::{
    runtime::AppState,
    store::{conference::Member, conference_phone::PhoneMove},
    ApiError, Result,
};
use rvoip_core::{
    events::Event,
    ids::{BridgeId, ConnectionId, ParticipantId, SessionId},
    Transport,
};
use serde_json::{json, Value};
use std::time::Duration;

const PENDING: &[&str] = &["prepared", "dialing", "answered", "confirmed"];

pub async fn move_to_phone(
    state: &AppState,
    tenant: &str,
    cid: &str,
    sid: &str,
    owner: &Member,
    source: &str,
    request: &str,
) -> Result<Value> {
    if owner.role != "owner" {
        return Err(ApiError::forbidden(
            "only the owner may move their audio to a phone",
        ));
    }
    let endpoint = owner
        .sip
        .as_deref()
        .ok_or_else(|| ApiError::conflict("owner has no provisioned telephone route"))?;
    // No model/client-supplied dial destination: this route belongs to the
    // authenticated Conversation member and was provisioned by the operator.
    let _guard = state.conference_handoff.lock().await;
    let voice = state
        .store
        .conference_voice_for_session(tenant, sid)?
        .filter(|v| v.conversation_id == cid)
        .ok_or_else(|| ApiError::not_found("voice Session not in this Conversation"))?;
    state
        .store
        .require_conference_browser(tenant, cid, sid, &owner.participant_id, source)?;
    let route = state
        .store
        .speaking_route(tenant, sid)?
        .filter(|r| r.connection_id == source && r.participant_id == owner.participant_id)
        .ok_or_else(|| ApiError::conflict("owner browser must be the current speaking peer"))?;
    if state
        .orchestrator
        .bridge_peer_of(&ConnectionId::from_string(source))
        != Some(ConnectionId::from_string(&voice.remote_connection_id))
    {
        return Err(ApiError::conflict(
            "original speaking bridge is unavailable",
        ));
    }
    let ticket = state
        .orchestrator
        .prepare_outbound_connection(
            rvoip_core::OriginateRequest::new(
                SessionId::from_string(sid),
                ParticipantId::from_string(&owner.participant_id),
                endpoint,
                rvoip_core::Direction::Outbound,
                Default::default(),
            )
            .with_transport(Transport::Sip),
        )
        .await
        .map_err(|_| ApiError::internal("telephone connection preparation failed"))?;
    let connid = ticket.connection_id().to_string();
    state
        .store
        .prepare_phone_move(tenant, cid, sid, owner, &route, &connid, request)?;
    let phone = state
        .store
        .phone_move(tenant, &connid)?
        .ok_or_else(|| ApiError::internal("phone preparation missing"))?;
    let retained_connid = voice.remote_connection_id.clone();
    let background = state.clone();
    let tenant = tenant.to_owned();
    tokio::spawn(async move {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(45);
        let activation = tokio::time::timeout_at(deadline, ticket.commit()).await;
        if !matches!(activation, Ok(Ok(_))) {
            fail(
                &background,
                &tenant,
                &phone,
                "telephone dialing failed or timed out",
            )
            .await;
            return;
        }
        let mut prompt = None;
        let mut tick = tokio::time::interval(Duration::from_millis(20));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            let current = match background.store.phone_move(&tenant, &phone.connection_id) {
                Ok(Some(p)) => p,
                _ => {
                    fail(&background, &tenant, &phone, "phone state unavailable").await;
                    return;
                }
            };
            if current.state == "confirmed" {
                if let Some(prompt) = prompt.take() {
                    if let Err(reason) =
                        crate::conference_phone_prompt::PhonePrompt::stop(prompt, deadline).await
                    {
                        fail(&background, &tenant, &phone, reason).await;
                        return;
                    }
                }
                break;
            }
            if !PENDING.contains(&current.state.as_str()) {
                return;
            }
            if tokio::time::Instant::now() >= deadline {
                fail(
                    &background,
                    &tenant,
                    &phone,
                    "answer and press-1 confirmation timed out",
                )
                .await;
                return;
            }
            if background
                .store
                .get_session(&tenant, &phone.session_id)
                .ok()
                .flatten()
                .is_none_or(|s| s.state != "active")
            {
                fail(
                    &background,
                    &tenant,
                    &phone,
                    "voice Session ended during callback",
                )
                .await;
                return;
            }
            if current.state == "answered" && prompt.is_none() {
                let stream = background
                    .orchestrator
                    .wait_for_stream(
                        ConnectionId::from_string(&phone.connection_id),
                        rvoip_core::stream::StreamSelector::new(
                            rvoip_core::stream::StreamKind::Audio,
                        )
                        .with_readiness(rvoip_core::stream::MediaReadiness::Bidirectional),
                        deadline.min(tokio::time::Instant::now() + Duration::from_secs(10)),
                        Default::default(),
                    )
                    .await;
                let started = stream
                    .map_err(|_| "callback prompt audio did not become ready")
                    .and_then(crate::conference_phone_prompt::PhonePrompt::new);
                match started {
                    Ok(started) => prompt = Some(started),
                    Err(reason) => {
                        fail(&background, &tenant, &phone, reason).await;
                        return;
                    }
                }
                // Recheck cancellation/confirmation after the readiness wait.
                continue;
            }
            if let Some(prompt) = prompt.as_mut() {
                if let Err(reason) = prompt.send_next(deadline).await {
                    fail(&background, &tenant, &phone, reason).await;
                    return;
                }
            }
            tick.tick().await;
        }
        let ready = background
            .orchestrator
            .wait_for_stream(
                ConnectionId::from_string(&phone.connection_id),
                rvoip_core::stream::StreamSelector::new(rvoip_core::stream::StreamKind::Audio)
                    .with_readiness(rvoip_core::stream::MediaReadiness::Bidirectional),
                deadline,
                Default::default(),
            )
            .await;
        if ready.is_err() {
            fail(
                &background,
                &tenant,
                &phone,
                "telephone audio did not become ready",
            )
            .await;
            return;
        }
        let _guard = background.conference_handoff.lock().await;
        let active = background
            .store
            .get_session(&tenant, &phone.session_id)
            .ok()
            .flatten()
            .is_some_and(|s| s.state == "active");
        let claimed = active
            && background
                .store
                .transition_phone_move(
                    &tenant,
                    &phone.connection_id,
                    &["confirmed"],
                    "committing",
                    json!({}),
                )
                .unwrap_or(false);
        if !claimed {
            fail(&background, &tenant, &phone, "phone move no longer active").await;
            return;
        }
        let result = background
            .orchestrator
            .replace_bridge_destination(
                BridgeId::from_string(&phone.source_bridge_id),
                ConnectionId::from_string(&voice.remote_connection_id),
                ConnectionId::from_string(&phone.source_connection_id),
                ConnectionId::from_string(&phone.connection_id),
            )
            .await;
        match result {
            Ok(replaced) => {
                if background
                    .store
                    .commit_phone_move(
                        &tenant,
                        &phone,
                        &voice.remote_connection_id,
                        &replaced.bridge_id.to_string(),
                    )
                    .is_err()
                {
                    // The media effect already committed. Preserve the phone
                    // call and report ambiguity instead of hanging it up or
                    // redialing in response to a database failure.
                    let _ = background.store.transition_phone_move(
                        &tenant,
                        &phone.connection_id,
                        &["committing"],
                        "unknown",
                        json!({"reason":"media moved; durable result needs reconciliation"}),
                    );
                    return;
                }
                let _ = background
                    .orchestrator
                    .end_connection(
                        ConnectionId::from_string(&phone.source_connection_id),
                        rvoip_core::EndReason::Normal,
                    )
                    .await;
            }
            Err(_) => {
                fail(
                    &background,
                    &tenant,
                    &phone,
                    "phone handoff failed; original route retained if available",
                )
                .await
            }
        }
    });
    Ok(
        json!({"sid":sid,"connid":connid,"state":"prepared","confirmation":"answer and press 1","retained_connid":retained_connid}),
    )
}

async fn fail(state: &AppState, tenant: &str, phone: &PhoneMove, reason: &str) {
    let _ = state.store.transition_phone_move(
        tenant,
        &phone.connection_id,
        &["prepared", "dialing", "answered", "confirmed", "committing"],
        "failed",
        json!({"reason":reason}),
    );
    let ended = state
        .orchestrator
        .end_connection(
            ConnectionId::from_string(&phone.connection_id),
            rvoip_core::EndReason::Normal,
        )
        .await;
    if !matches!(
        ended,
        Ok(()) | Err(rvoip_core::RvoipError::ConnectionNotFound(_))
    ) {
        let _ = state.store.transition_phone_move(
            tenant,
            &phone.connection_id,
            &["failed", "cancelled"],
            "unknown",
            json!({"reason":"callback termination needs verification; do not redial"}),
        );
    }
}

pub async fn cancel(
    state: &AppState,
    tenant: &str,
    cid: &str,
    sid: &str,
    owner: &Member,
    connid: &str,
) -> Result<Value> {
    if owner.role != "owner" {
        return Err(ApiError::forbidden(
            "only the owner may cancel their phone move",
        ));
    }
    let _guard = state.conference_handoff.lock().await;
    let phone = state
        .store
        .phone_move(tenant, connid)?
        .filter(|p| {
            p.conversation_id == cid
                && p.session_id == sid
                && p.participant_id == owner.participant_id
        })
        .ok_or_else(|| {
            ApiError::forbidden("phone move is not owned by this participant in this Session")
        })?;
    if phone.state == "cancelled" {
        return Ok(json!({"sid":sid,"connid":connid,"state":"cancelled"}));
    }
    if !state
        .store
        .transition_phone_move(tenant, connid, PENDING, "cancelled", json!({}))?
    {
        return Err(ApiError::conflict(
            "phone move already finished; use session.end to end voice",
        ));
    }
    let ended = state
        .orchestrator
        .end_connection(
            ConnectionId::from_string(connid),
            rvoip_core::EndReason::Normal,
        )
        .await;
    if !matches!(
        ended,
        Ok(()) | Err(rvoip_core::RvoipError::ConnectionNotFound(_))
    ) {
        state.store.transition_phone_move(
            tenant,
            connid,
            &["cancelled"],
            "unknown",
            json!({"reason":"callback termination needs verification; do not redial"}),
        )?;
        return Err(ApiError::internal(
            "callback termination needs verification",
        ));
    }
    Ok(json!({"sid":sid,"connid":connid,"state":"cancelled"}))
}

pub fn mirror(state: &AppState, event: &Event) -> Result<()> {
    let tenant = &state.config.tenant_id;
    let (connid, next, allowed) = match event {
        Event::ConnectionOutbound { connection_id, .. } => {
            (connection_id, "dialing", &["prepared"][..])
        }
        Event::ConnectionConnected { connection_id, .. } => {
            (connection_id, "answered", &["prepared", "dialing"][..])
        }
        Event::DtmfReceived {
            connection_id,
            digits,
            ..
        } if digits == "1" => (connection_id, "confirmed", &["answered"][..]),
        Event::ConnectionEnded { connection_id, .. } => (
            connection_id,
            "ended",
            &[
                "prepared",
                "dialing",
                "answered",
                "confirmed",
                "committing",
                "speaking",
            ][..],
        ),
        Event::ConnectionFailed { connection_id, .. } => (
            connection_id,
            "failed",
            &[
                "prepared",
                "dialing",
                "answered",
                "confirmed",
                "committing",
                "speaking",
            ][..],
        ),
        _ => return Ok(()),
    };
    let connection = connid.to_string();
    if matches!(next, "ended" | "failed") {
        // Serialize termination with the media commit so a hangup cannot
        // turn a just-committed route into an untracked second call.
        let state = state.clone();
        let tenant = tenant.to_owned();
        tokio::spawn(async move {
            let _guard = state.conference_handoff.lock().await;
            let Ok(Some(phone)) = state.store.phone_move(&tenant, &connection) else {
                return;
            };
            if state
                .store
                .transition_phone_move(
                    &tenant,
                    &connection,
                    &[
                        "prepared",
                        "dialing",
                        "answered",
                        "confirmed",
                        "committing",
                        "speaking",
                    ],
                    next,
                    json!({}),
                )
                .unwrap_or(false)
                && phone.state == "speaking"
            {
                let _ = crate::conference_voice::terminate(
                    &state,
                    &SessionId::from_string(phone.session_id),
                )
                .await;
            }
        });
    } else {
        state
            .store
            .transition_phone_move(tenant, &connection, allowed, next, json!({}))?;
    }
    Ok(())
}
