//! Experimental conversation-control/1: typed, authenticated UCTP operations.
use crate::runtime::AppState;
use crate::store::conference::{Member, MemberInput};
use crate::{ApiError, Result};
use rvoip_core::ids::{ConversationId, TenantId};
use rvoip_uctp::application::{ApplicationContext, ApplicationError, ApplicationHandler};
use rvoip_uctp::envelope::UctpEnvelope;
use rvoip_uctp::types::MessageType;
use serde_json::{json, Value};
use std::collections::HashSet;
use tokio::sync::Mutex;
use uuid::Uuid;

pub const PROFILE: &str = "conversation-control/1";

pub struct Commands {
    state: AppState,
    // Serializes check/begin/execute/finish across peers in this single-server
    // deployment. Provider effects run from the durable outbox, not this lock.
    execution: Mutex<()>,
    observers: crate::uctp_observer::Observers,
}

impl Commands {
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            execution: Mutex::new(()),
            observers: Default::default(),
        }
    }

    fn tenant<'a>(&self, context: &'a ApplicationContext) -> Result<&'a str> {
        context
            .principal
            .tenant
            .as_deref()
            .filter(|t| *t == self.state.config.tenant_id)
            .ok_or_else(|| ApiError::forbidden("tenant not admitted"))
    }

    fn member(&self, tenant: &str, cid: &str, context: &ApplicationContext) -> Result<Member> {
        self.state
            .store
            .conference_members(tenant, cid)?
            .into_iter()
            .find(|m| m.subject == context.principal.subject)
            .ok_or_else(|| ApiError::forbidden("Conversation membership required"))
    }

    fn fingerprint(request: &UctpEnvelope) -> String {
        blake3::hash(json!({"type":request.msg_type,"cid":request.cid,"sid":request.sid,"connid":request.connid,"payload":request.payload}).to_string().as_bytes()).to_hex().to_string()
    }

    fn capabilities(&self) -> Value {
        let dependencies: Value =
            serde_json::from_str(include_str!("../config/conference-dependencies.json"))
                .expect("compiled dependency manifest");
        let voice = crate::conference_voice::available(&self.state);
        let mut operations = vec![
            "conversation.subscribe",
            "conversation.inspect",
            "conversation.preflight",
            "conversation.close",
            "message.send",
            "message.history",
            "session.update",
        ];
        if voice {
            operations.extend(["session.invite", "session.end"]);
        }
        if cfg!(feature = "media-webrtc") {
            operations.extend(["connection.answer", "connection.end"]);
        }
        json!({
            "implementation": {
                "host": "parley", "host_version": env!("CARGO_PKG_VERSION"),
                "rvoip_baseline": dependencies["rvoip"]["version"],
                "rvoip_revision": dependencies["rvoip"]["revision"],
                "rvoip_patch_sha256": dependencies["rvoip"]["patch_sha256"],
                "rvoip_patched": true, "profile": PROFILE, "experimental": true,
                "envelope_version": 1, "control_transport": "websocket"
            },
            "operations": operations, "delivery": ["chat", "sms"],
            "sms_mode": if self.state.config.vapi_chat_mode == "fake" { "fake" } else { "telnyx" },
            "sms_configured": self.state.config.vapi_chat_mode == "fake" ||
                (self.state.telnyx.is_some() && self.state.telnyx_verifier.is_some() && !self.state.config.telnyx_from.is_empty()),
            "voice": voice,
            "assistant_voice": crate::conference_voice::assistant_available(&self.state),
            "browser_handoff": cfg!(feature = "media-webrtc"),
            "phone_handoff": cfg!(feature = "media-webrtc") && voice,
        })
    }

    fn cached(
        &self,
        context: &ApplicationContext,
        request: &UctpEnvelope,
    ) -> Result<Option<UctpEnvelope>> {
        let tenant = self.tenant(context)?;
        // Revalidate membership even when returning a cached outcome.
        if let Some(cid) = request.cid.as_deref() {
            self.member(tenant, cid, context)?;
        }
        match self.state.store.cached_command(
            tenant,
            &context.principal.subject,
            &request.id,
            &Self::fingerprint(request),
        )? {
            None => Ok(None),
            Some(None) => Err(ApiError::conflict(
                "operation outcome pending; inspect history before retrying effects",
            )),
            Some(Some(value)) => serde_json::from_value(value)
                .map(Some)
                .map_err(|_| ApiError::internal("invalid stored UCTP response")),
        }
    }

    async fn execute(
        &self,
        context: ApplicationContext,
        request: &UctpEnvelope,
    ) -> Result<UctpEnvelope> {
        let tenant = self.tenant(&context)?.to_owned();
        if request.msg_type == MessageType::ConversationCreate {
            if context.principal.subject != "api" || request.cid.is_some() {
                return Err(ApiError::forbidden(
                    "provisioning requires the administrator and no existing cid",
                ));
            }
            let participants: Vec<MemberInput> = serde_json::from_value(
                request
                    .payload
                    .get("participants")
                    .cloned()
                    .unwrap_or(Value::Null),
            )
            .map_err(|_| ApiError::bad_request("participants required"))?;
            crate::store::conference::validate_members(&participants)?;
            let cid = format!("conv_{}", Uuid::new_v4().simple());
            self.state
                .orchestrator
                .open_conversation_with_id(
                    ConversationId::from_string(cid.clone()),
                    TenantId::from_string(tenant.clone()),
                    rvoip_core::conversation::ConversationPolicy::Persistent,
                    Default::default(),
                )
                .await
                .map_err(|e| ApiError::internal(format!("open Conversation: {e}")))?;
            let members =
                self.state
                    .store
                    .create_conference(&tenant, &cid, &participants, &request.id)?;
            return Ok(UctpEnvelope::new(
                MessageType::ConversationOpened,
                json!({"profile":PROFILE,"participants":members,"policy":"persistent"}),
            )
            .with_cid(cid));
        }
        if let MessageType::Unknown(kind) = &request.msg_type {
            if matches!(kind.as_str(), "inbox.list" | "inbox.resolve") {
                if context.principal.subject != "api"
                    || request.cid.is_some()
                    || request.sid.is_some()
                    || request.connid.is_some()
                {
                    return Err(ApiError::forbidden(
                        "holding inbox requires administrator and no Conversation binding",
                    ));
                }
                if kind == "inbox.list" {
                    let after = cursor_after(&request.payload)?;
                    let entries = self.state.store.held_conference_sms(&tenant, after)?;
                    let cursor = entries.last().map(|e| e.id).unwrap_or(after);
                    return Ok(UctpEnvelope::new(
                        MessageType::Ack,
                        json!({"profile":PROFILE,"entries":entries,"cursor":cursor}),
                    ));
                }
                let id = request
                    .payload
                    .get("inbox_id")
                    .and_then(Value::as_i64)
                    .filter(|id| *id > 0)
                    .ok_or_else(|| ApiError::bad_request("positive inbox_id required"))?;
                let row = self.state.store.resolve_conference_sms(
                    &tenant,
                    id,
                    required_string(&request.payload, "conversation_id")?,
                    required_string(&request.payload, "participant_id")?,
                    required_string(&request.payload, "verification_note")?,
                    &context.principal.subject,
                    &request.id,
                )?;
                crate::events::publish(
                    &self.state,
                    &tenant,
                    Some(&row.conversation_id),
                    "message.received",
                );
                return Ok(UctpEnvelope::new(
                    MessageType::Ack,
                    json!({"profile":PROFILE,"message":row}),
                ));
            }
        }
        let cid = request
            .cid
            .as_deref()
            .ok_or_else(|| ApiError::bad_request("cid required"))?;
        let member = self.member(&tenant, cid, &context)?;
        // Closed Conversations remain readable/replayable, but cannot create effects.
        let closed = self
            .state
            .store
            .get_conversation(&tenant, cid)?
            .is_some_and(|row| row.state != "open");
        let read_only = matches!(
            &request.msg_type,
            MessageType::MessageHistory | MessageType::ConversationClose
        ) || matches!(&request.msg_type, MessageType::Unknown(kind) if matches!(kind.as_str(), "conversation.subscribe" | "conversation.inspect" | "conversation.preflight"));
        if closed && !read_only {
            return Err(ApiError::conflict("Conversation is closed"));
        }
        match &request.msg_type {
            MessageType::Unknown(kind) if kind == "conversation.preflight" => {
                if !member.observes_all() {
                    return Err(ApiError::forbidden("preflight requires owner or assistant"));
                }
                let readiness = self.state.store.conference_preflight(&tenant, cid)?;
                Ok(UctpEnvelope::new(MessageType::Ack, json!({"profile":PROFILE,"readiness":readiness,"capabilities":self.capabilities()})).with_cid(cid))
            }
            MessageType::ConversationClose => {
                if member.role != "owner" || request.sid.is_some() || request.connid.is_some() {
                    return Err(ApiError::forbidden("Conversation close requires its owner and no Session or Connection binding"));
                }
                self.state.store.close_conference(
                    &tenant,
                    cid,
                    &member.participant_id,
                    required_string(&request.payload, "verification_note")?,
                    &request.id,
                )?;
                // There are no active Sessions; this only retires the in-memory shell.
                if let Err(error) = self
                    .state
                    .orchestrator
                    .close_conversation(ConversationId::from_string(cid), false)
                    .await
                {
                    tracing::warn!(%error,"closed Conversation persisted; core shell cleanup unavailable");
                }
                Ok(UctpEnvelope::new(
                    MessageType::ConversationClosed,
                    json!({"profile":PROFILE,"state":"closed"}),
                )
                .with_cid(cid))
            }
            MessageType::Unknown(kind) if kind == "conversation.inspect" => {
                if !member.observes_all() {
                    return Err(ApiError::forbidden(
                        "command inspection requires owner or assistant",
                    ));
                }
                let evidence = self.state.store.inspect_conference_command(
                    &tenant,
                    cid,
                    required_string(&request.payload, "request_id")?,
                )?;
                Ok(UctpEnvelope::new(
                    MessageType::Ack,
                    json!({"profile":PROFILE,"evidence":evidence}),
                )
                .with_cid(cid))
            }
            #[cfg(feature = "media-webrtc")]
            MessageType::ConnectionEnd => {
                let sid = request
                    .sid
                    .as_deref()
                    .ok_or_else(|| ApiError::bad_request("sid required"))?;
                let connid = request
                    .connid
                    .as_deref()
                    .ok_or_else(|| ApiError::bad_request("connid required"))?;
                self.state.store.require_conference_browser(
                    &tenant,
                    cid,
                    sid,
                    &member.participant_id,
                    connid,
                )?;
                self.state
                    .orchestrator
                    .end_connection(
                        rvoip_core::ids::ConnectionId::from_string(connid),
                        rvoip_core::EndReason::Normal,
                    )
                    .await
                    .map_err(|e| ApiError::internal(format!("end browser: {e}")))?;
                self.state.store.update_conference_browser(
                    &tenant,
                    connid,
                    "ended",
                    Some(&request.id),
                    json!({}),
                )?;
                Ok(UctpEnvelope::new(
                    MessageType::Ack,
                    json!({"profile":PROFILE,"connid":connid,"state":"ended"}),
                )
                .with_cid(cid))
            }
            MessageType::SessionUpdate
                if request.payload.get("kind").and_then(Value::as_str) == Some("confirm_ended") =>
            {
                let sid = request
                    .sid
                    .as_deref()
                    .ok_or_else(|| ApiError::bad_request("sid required"))?;
                let result = self.state.store.confirm_interrupted_voice_ended(
                    &tenant,
                    cid,
                    sid,
                    &member,
                    required_string(&request.payload, "verification_note")?,
                    &request.id,
                )?;
                Ok(UctpEnvelope::new(
                    MessageType::Ack,
                    json!({"profile":PROFILE,"session":result}),
                )
                .with_cid(cid))
            }
            #[cfg(feature = "media-webrtc")]
            MessageType::SessionUpdate => {
                let sid = request
                    .sid
                    .as_deref()
                    .ok_or_else(|| ApiError::bad_request("sid required"))?;
                match required_string(&request.payload, "kind")? {
                    "move_to_phone" | "cancel_phone_move" => {
                        if member.role != "owner" {
                            return Err(ApiError::forbidden("phone moves require the owner"));
                        }
                        if request
                            .payload
                            .as_object()
                            .is_none_or(|p| p.keys().any(|k| k != "kind" && k != "profile"))
                        {
                            return Err(ApiError::bad_request("phone move uses only the provisioned owner route; no dial destination accepted"));
                        }
                        let connid = request
                            .connid
                            .as_deref()
                            .ok_or_else(|| ApiError::bad_request("connid required"))?;
                        let result = if request.payload["kind"] == "move_to_phone" {
                            crate::conference_phone::move_to_phone(
                                &self.state,
                                &tenant,
                                cid,
                                sid,
                                &member,
                                connid,
                                &request.id,
                            )
                            .await?
                        } else {
                            crate::conference_phone::cancel(
                                &self.state,
                                &tenant,
                                cid,
                                sid,
                                &member,
                                connid,
                            )
                            .await?
                        };
                        let phone_connid = result["connid"].as_str().unwrap().to_owned();
                        Ok(UctpEnvelope::new(
                            MessageType::Ack,
                            json!({"profile":PROFILE,"session":result}),
                        )
                        .with_cid(cid)
                        .with_sid(sid)
                        .with_connid(phone_connid))
                    }
                    "join_browser" => {
                        let result = crate::conference_voice::join_browser(
                            &self.state,
                            &tenant,
                            cid,
                            sid,
                            &member,
                            &request.id,
                        )
                        .await?;
                        let connid = result["connid"].as_str().unwrap().to_string();
                        let mut payload = result;
                        payload["profile"] = json!(PROFILE);
                        Ok(UctpEnvelope::new(MessageType::ConnectionOffer, payload)
                            .with_cid(cid)
                            .with_sid(sid)
                            .with_connid(connid))
                    }
                    "handoff_to_browser" => {
                        let connid = request
                            .connid
                            .as_deref()
                            .ok_or_else(|| ApiError::bad_request("connid required"))?;
                        let result = crate::conference_voice::handoff_browser(
                            &self.state,
                            &tenant,
                            cid,
                            sid,
                            &member,
                            connid,
                            &request.id,
                        )
                        .await?;
                        Ok(UctpEnvelope::new(
                            MessageType::Ack,
                            json!({"profile":PROFILE,"session":result}),
                        )
                        .with_cid(cid))
                    }
                    _ => Err(ApiError::bad_request("unknown session.update kind")),
                }
            }
            #[cfg(feature = "media-webrtc")]
            MessageType::ConnectionAnswer => {
                let sid = request
                    .sid
                    .as_deref()
                    .ok_or_else(|| ApiError::bad_request("sid required"))?;
                let connid = request
                    .connid
                    .as_deref()
                    .ok_or_else(|| ApiError::bad_request("connid required"))?;
                let setup = request
                    .payload
                    .get("substrate_setup")
                    .ok_or_else(|| ApiError::bad_request("substrate_setup required"))?;
                if setup.get("sdp_type").and_then(Value::as_str) != Some("answer") {
                    return Err(ApiError::bad_request("answer SDP required"));
                }
                let result = crate::conference_voice::answer_browser(
                    &self.state,
                    &tenant,
                    cid,
                    sid,
                    &member,
                    connid,
                    required_string(setup, "sdp")?,
                    &request.id,
                )
                .await?;
                Ok(UctpEnvelope::new(
                    MessageType::Ack,
                    json!({"profile":PROFILE,"connection":result}),
                )
                .with_cid(cid))
            }
            MessageType::SessionInvite => {
                if request.sid.is_some()
                    || request.payload.get("medium").and_then(Value::as_str) != Some("voice")
                {
                    return Err(ApiError::bad_request(
                        "outbound invitation requires medium voice and no sid",
                    ));
                }
                let target = required_string(&request.payload, "to")?;
                let target = self
                    .state
                    .store
                    .conference_members(&tenant, cid)?
                    .into_iter()
                    .find(|m| m.participant_id == target)
                    .ok_or_else(|| ApiError::forbidden("target is not a Conversation member"))?;
                let result = crate::conference_voice::invite(
                    &self.state,
                    &tenant,
                    cid,
                    &member,
                    &target,
                    required_string(&request.payload, "purpose")?,
                    &request.id,
                )
                .await?;
                Ok(UctpEnvelope::new(
                    MessageType::Ack,
                    json!({"profile":PROFILE,"session":result}),
                )
                .with_cid(cid))
            }
            MessageType::SessionEnd => {
                let sid = request
                    .sid
                    .as_deref()
                    .ok_or_else(|| ApiError::bad_request("sid required"))?;
                let result = crate::conference_voice::end(
                    &self.state,
                    &tenant,
                    cid,
                    sid,
                    &member,
                    &request.id,
                )
                .await?;
                Ok(UctpEnvelope::new(
                    MessageType::Ack,
                    json!({"profile":PROFILE,"session":result}),
                )
                .with_cid(cid))
            }
            MessageType::MessageSend => {
                let ids: Vec<String> = serde_json::from_value(
                    request.payload.get("to").cloned().unwrap_or(Value::Null),
                )
                .map_err(|_| ApiError::bad_request("to must be explicit participant IDs"))?;
                let distinct: HashSet<_> = ids.iter().collect();
                if ids.is_empty() || ids.len() > 32 || ids.len() != distinct.len() {
                    return Err(ApiError::bad_request(
                        "recipient list empty, duplicated, or too large",
                    ));
                }
                let roster = self.state.store.conference_members(&tenant, cid)?;
                let recipients = ids
                    .iter()
                    .map(|id| {
                        roster
                            .iter()
                            .find(|m| &m.participant_id == id)
                            .cloned()
                            .ok_or_else(|| {
                                ApiError::forbidden("recipient is not in this Conversation")
                            })
                    })
                    .collect::<Result<Vec<_>>>()?;
                let medium = request
                    .payload
                    .get("delivery")
                    .and_then(Value::as_str)
                    .unwrap_or("chat");
                if medium == "sms" && !member.observes_all() {
                    return Err(ApiError::forbidden(
                        "only owner or delegated assistant can send outbound SMS",
                    ));
                }
                let msg_id = required_string(&request.payload, "msg_id")?;
                if msg_id.len() > 128
                    || !msg_id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                {
                    return Err(ApiError::bad_request("invalid msg_id"));
                }
                let result = self.state.store.enqueue_conference_message(
                    &tenant,
                    cid,
                    &member,
                    &recipients,
                    msg_id,
                    required_string(&request.payload, "body")?,
                    request
                        .payload
                        .get("content_type")
                        .and_then(Value::as_str)
                        .unwrap_or("text/plain"),
                    medium,
                    request
                        .payload
                        .get("in_reply_to_msg")
                        .and_then(Value::as_str),
                    &request.id,
                    &self.state.config.telnyx_from,
                )?;
                crate::events::publish(&self.state, &tenant, Some(cid), "message.accepted");
                Ok(UctpEnvelope::new(
                    MessageType::Ack,
                    json!({"profile":PROFILE,"message":result}),
                )
                .with_cid(cid))
            }
            MessageType::MessageHistory => {
                let page = self.state.store.conference_history_page(
                    &tenant,
                    cid,
                    &member,
                    cursor_after(&request.payload)?,
                )?;
                Ok(UctpEnvelope::new(
                    MessageType::MessageHistory,
                    json!({"profile":PROFILE,"messages":page.messages,"cursor":page.cursor,"has_more":page.has_more}),
                )
                .with_cid(cid))
            }
            MessageType::Unknown(kind) if kind == "conversation.subscribe" => {
                let after = cursor_after(&request.payload)?;
                let events = self
                    .state
                    .store
                    .conference_events(&tenant, cid, &member, after, 500)?;
                let cursor = events.last().map(|e| e.seq).unwrap_or(after);
                let subscription = if request
                    .payload
                    .get("live")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    Some(self.observers.subscribe(
                        self.state.clone(),
                        context,
                        tenant.clone(),
                        cid.to_string(),
                        cursor,
                    )?)
                } else {
                    None
                };
                let mut members = self.state.store.conference_members(&tenant, cid)?;
                if !member.observes_all() {
                    for m in &mut members {
                        if m.participant_id != member.participant_id {
                            m.sms = None;
                            m.sip = None;
                        }
                    }
                }
                Ok(UctpEnvelope::new(
                    MessageType::Unknown("conversation.snapshot".into()),
                    json!({"profile":PROFILE,"state":if closed {"closed"} else {"open"},"participants":members,"events":events,"cursor":cursor,
                        "capabilities":self.capabilities(),"subscription":subscription
                    }),
                )
                .with_cid(cid))
            }
            _ => Err(ApiError::new(
                axum::http::StatusCode::NOT_IMPLEMENTED,
                "unsupported-command",
                "Unsupported command",
                "command is not implemented by conversation-control/1",
            )),
        }
    }
}

fn cursor_after(payload: &Value) -> Result<i64> {
    match payload.get("after") {
        None => Ok(0),
        Some(value) => value
            .as_i64()
            .filter(|n| *n >= 0)
            .ok_or_else(|| ApiError::bad_request("after must be a nonnegative integer")),
    }
}

fn required_string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ApiError::bad_request(format!("{key} required")))
}

fn application_error(error: ApiError) -> ApplicationError {
    // Internal implementation diagnostics stay server-side.
    if error.status.is_server_error() && error.status != axum::http::StatusCode::NOT_IMPLEMENTED {
        tracing::error!(detail=%error.detail,"UCTP application error");
        ApplicationError::new(error.status.as_u16(), "application operation failed")
    } else {
        ApplicationError::new(error.status.as_u16(), error.detail)
    }
}

#[async_trait::async_trait]
impl ApplicationHandler for Commands {
    fn profile(&self) -> &'static str {
        PROFILE
    }

    async fn handle(
        &self,
        context: ApplicationContext,
        request: UctpEnvelope,
    ) -> std::result::Result<UctpEnvelope, ApplicationError> {
        let _guard = self.execution.lock().await;
        if let Some(response) = self.cached(&context, &request).map_err(application_error)? {
            return Ok(response);
        }
        let tenant = self.tenant(&context).map_err(application_error)?.to_owned();
        let subject = context.principal.subject.clone();
        self.state
            .store
            .begin_command(
                &tenant,
                &subject,
                &request.id,
                &Self::fingerprint(&request),
                serde_json::to_value(&request)
                    .map_err(|_| ApplicationError::new(500, "request encoding failed"))?,
            )
            .map_err(application_error)?;
        let mut response = match self.execute(context, &request).await {
            Ok(response) => response,
            Err(error) => {
                let error = application_error(error);
                UctpEnvelope::new(
                    MessageType::Error,
                    json!({"code":error.code,"category":"application","reason":error.reason}),
                )
            }
        };
        response.in_reply_to = Some(request.id.clone());
        if response.cid.is_none() {
            response.cid = request.cid.clone();
        }
        if response.sid.is_none() {
            response.sid = request.sid.clone();
        }
        if response.connid.is_none() {
            response.connid = request.connid.clone();
        }
        self.state
            .store
            .finish_command(
                &tenant,
                &subject,
                &request.id,
                serde_json::to_value(&response)
                    .map_err(|_| ApplicationError::new(500, "response encoding failed"))?,
            )
            .map_err(application_error)?;
        Ok(response)
    }

    async fn replay(
        &self,
        context: ApplicationContext,
        request: UctpEnvelope,
    ) -> std::result::Result<UctpEnvelope, ApplicationError> {
        let _guard = self.execution.lock().await;
        self.cached(&context, &request)
            .map_err(application_error)?
            .ok_or_else(|| ApplicationError::new(409, "request outcome unavailable"))
    }
}
