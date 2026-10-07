use crate::error::{ApiError, Result};
use crate::identity::{self, IngressKeys, Match};
use crate::runtime::AppState;
use crate::store::{ConversationRow, MessageRow, ParticipantRow, SessionRow};
use crate::vcon_wrap;
use chrono::Utc;
use rvoip_core::conversation::ConversationPolicy;
use rvoip_core::ids::{ConversationId, ParticipantId, SessionId, TenantId};
use rvoip_core::session::SessionMedium;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize)]
pub struct CreateConversation {
    pub identity: IngressKeys,
    #[serde(default = "default_policy")]
    pub policy: String,
    #[serde(default)]
    pub participants: Vec<Value>,
}

fn default_policy() -> String {
    "persistent".into()
}

#[derive(Clone, Debug, Serialize)]
pub struct ConversationView {
    pub id: String,
    pub tenant_id: String,
    pub state: String,
    pub policy: String,
    pub opened_at: String,
    pub closed_at: Option<String>,
    pub last_activity_at: String,
    pub match_kind: String,
    pub participants: Vec<ParticipantRow>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct PostMessage {
    pub medium: String,
    pub sender_participant_id: Option<String>,
    pub body: String,
    /// When true, persist only — do not send via the SMS provider (inbound).
    #[serde(default)]
    pub inbound: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PostSession {
    pub medium: String,
    #[serde(default)]
    pub direction: Option<String>,
}

pub async fn create_or_continue(
    state: &AppState,
    tenant_id: &str,
    req: CreateConversation,
) -> Result<ConversationView> {
    let keys = req.identity;
    let matched = identity::resolve_ingress(
        &state.store,
        tenant_id,
        &keys,
        state.config.reopen_window_secs,
        Utc::now(),
    )?;
    match matched {
        Match::Continue(cid) => {
            ensure_orchestrator_open(state, tenant_id, &cid).await?;
            state.store.touch_conversation(tenant_id, &cid)?;
            crate::events::emit(
                state,
                tenant_id,
                Some(&cid),
                "conversation.continued",
                json!({ "cid": cid }),
            )?;
            view(state, tenant_id, &cid, "continue")
        }
        Match::Reopen(cid) => {
            state.store.reopen_conversation(tenant_id, &cid)?;
            reopen_orchestrator(state, tenant_id, &cid).await?;
            crate::events::emit(
                state,
                tenant_id,
                Some(&cid),
                "conversation.continued",
                json!({ "cid": cid }),
            )?;
            view(state, tenant_id, &cid, "reopen")
        }
        Match::OpenNew => open_new(state, tenant_id, &keys, &req.policy).await,
    }
}

async fn open_new(
    state: &AppState,
    tenant_id: &str,
    keys: &IngressKeys,
    policy: &str,
) -> Result<ConversationView> {
    let tenant = TenantId::from_string(tenant_id.to_string());
    let orch_policy = if policy == "ephemeral" {
        ConversationPolicy::Ephemeral {
            idle_close_secs: 30,
        }
    } else {
        ConversationPolicy::Persistent
    };
    let cid = state
        .orchestrator
        .open_conversation(tenant, orch_policy, Default::default())
        .await
        .map_err(|e| ApiError::internal(format!("open_conversation: {e}")))?;
    let now = Utc::now().to_rfc3339();
    let row = ConversationRow {
        id: cid.to_string(),
        tenant_id: tenant_id.to_string(),
        state: "open".into(),
        policy: if policy == "ephemeral" {
            "ephemeral".into()
        } else {
            "persistent".into()
        },
        opened_at: now.clone(),
        closed_at: None,
        last_activity_at: now.clone(),
        vapi_chat_session_id: None,
        metadata: json!({}),
    };
    state.store.insert_conversation(&row)?;
    for (key_type, key_value) in keys.iter() {
        state
            .store
            .upsert_identity(tenant_id, key_type, key_value, &row.id, false)?;
    }
    let customer = ParticipantRow {
        id: ParticipantId::new().to_string(),
        tenant_id: tenant_id.to_string(),
        conversation_id: row.id.clone(),
        kind: "human".into(),
        role: "customer".into(),
        identity_ref: keys.e164.clone().or(keys.visitor_id.clone()),
        display_name: None,
        joined_at: now.clone(),
        left_at: None,
    };
    state.store.insert_participant(&customer)?;
    let ai = ParticipantRow {
        id: ParticipantId::new().to_string(),
        tenant_id: tenant_id.to_string(),
        conversation_id: row.id.clone(),
        kind: "ai".into(),
        role: "agent".into(),
        identity_ref: None,
        display_name: Some("assistant".into()),
        joined_at: now,
        left_at: None,
    };
    state.store.insert_participant(&ai)?;
    crate::events::emit(
        state,
        tenant_id,
        Some(&row.id),
        "conversation.opened",
        json!({ "cid": row.id }),
    )?;
    crate::observe::conversation_opened();
    view(state, tenant_id, &row.id, "open")
}

async fn ensure_orchestrator_open(state: &AppState, tenant_id: &str, cid: &str) -> Result<()> {
    let tenant = TenantId::from_string(tenant_id.to_string());
    state
        .orchestrator
        .open_conversation_with_id(
            ConversationId::from_string(cid.to_string()),
            tenant,
            ConversationPolicy::Persistent,
            Default::default(),
        )
        .await
        .map_err(|e| ApiError::internal(format!("ensure conversation: {e}")))?;
    Ok(())
}

async fn reopen_orchestrator(state: &AppState, tenant_id: &str, cid: &str) -> Result<()> {
    let id = ConversationId::from_string(cid.to_string());
    match state.orchestrator.reopen_conversation(id.clone()).await {
        Ok(()) => Ok(()),
        Err(_) => ensure_orchestrator_open(state, tenant_id, cid).await,
    }
}

pub fn view(
    state: &AppState,
    tenant_id: &str,
    cid: &str,
    match_kind: &str,
) -> Result<ConversationView> {
    let row = state
        .store
        .get_conversation(tenant_id, cid)?
        .ok_or_else(|| ApiError::not_found("conversation not found").with_conversation(cid))?;
    let participants = state.store.list_participants(tenant_id, cid)?;
    Ok(ConversationView {
        id: row.id,
        tenant_id: row.tenant_id,
        state: row.state,
        policy: row.policy,
        opened_at: row.opened_at,
        closed_at: row.closed_at,
        last_activity_at: row.last_activity_at,
        match_kind: match_kind.into(),
        participants,
    })
}

pub async fn close(state: &AppState, tenant_id: &str, cid: &str) -> Result<ConversationView> {
    if !state.store.conference_members(tenant_id, cid)?.is_empty() {
        state.store.close_conference(
            tenant_id,
            cid,
            "administrator",
            "Closed through administrator API after local effects settled",
            &format!("env_{}", Uuid::new_v4().simple()),
        )?;
        let _ = state
            .orchestrator
            .close_conversation(ConversationId::from_string(cid), false)
            .await;
        return view(state, tenant_id, cid, "closed");
    }
    let id = ConversationId::from_string(cid.to_string());
    let _ = state.orchestrator.close_conversation(id, true).await;
    state.store.close_conversation(tenant_id, cid)?;
    crate::events::emit(
        state,
        tenant_id,
        Some(cid),
        "conversation.closed",
        json!({ "cid": cid }),
    )?;
    let _ = vcon_wrap::wrap_conversation(state, tenant_id, cid);
    view(state, tenant_id, cid, "closed")
}

pub fn post_message(
    state: &AppState,
    tenant_id: &str,
    cid: &str,
    req: PostMessage,
    idempotency_key: Option<&str>,
) -> Result<MessageRow> {
    let conv = state
        .store
        .get_conversation(tenant_id, cid)?
        .ok_or_else(|| ApiError::not_found("conversation not found").with_conversation(cid))?;
    if conv.state != "open" {
        return Err(ApiError::conflict("conversation is closed").with_conversation(cid));
    }
    if let Some(key) = idempotency_key {
        if let Some(existing) = state.store.idempotency_get(tenant_id, key)? {
            if let Some(row) = state.store.get_message(tenant_id, &existing)? {
                return Ok(row);
            }
        }
    }
    let medium = req.medium.to_lowercase();
    if medium != "sms" && medium != "chat" {
        return Err(ApiError::bad_request("medium must be sms or chat"));
    }
    let row = MessageRow {
        id: format!("msg_{}", Uuid::new_v4().simple()),
        tenant_id: tenant_id.to_string(),
        conversation_id: cid.to_string(),
        from_participant: req.sender_participant_id,
        medium,
        body: req.body,
        provider_id: None,
        state: "accepted".into(),
        created_at: Utc::now().to_rfc3339(),
    };
    state.store.insert_message(&row)?;
    state.store.touch_conversation(tenant_id, cid)?;
    crate::events::publish(state, tenant_id, Some(cid), "message.posted");
    if let Some(key) = idempotency_key {
        state.store.idempotency_put(tenant_id, key, &row.id)?;
    }
    if row.medium == "sms" && !req.inbound {
        crate::sms::send(state, tenant_id, cid, &row)?;
    }
    Ok(row)
}

pub async fn start_session(
    state: &AppState,
    tenant_id: &str,
    cid: &str,
    req: PostSession,
) -> Result<SessionRow> {
    let conv = state
        .store
        .get_conversation(tenant_id, cid)?
        .ok_or_else(|| ApiError::not_found("conversation not found").with_conversation(cid))?;
    if conv.state != "open" {
        return Err(ApiError::conflict("conversation is closed").with_conversation(cid));
    }
    let medium = req.medium.to_lowercase();
    let orch_medium = match medium.as_str() {
        "voice" | "pstn" => SessionMedium::Voice,
        "video" => SessionMedium::Video,
        "text" | "chat" | "sms" => SessionMedium::TextChat,
        _ => return Err(ApiError::bad_request("unknown session medium")),
    };
    if orch_medium == SessionMedium::Voice
        && state.store.count_live_voice_sessions(tenant_id, cid)? > 0
    {
        return Err(ApiError::conflict("voice session already live").with_conversation(cid));
    }
    ensure_orchestrator_open(state, tenant_id, cid).await?;
    let sid = state
        .orchestrator
        .start_session(
            ConversationId::from_string(cid.to_string()),
            orch_medium,
            Vec::new(),
        )
        .await
        .map_err(|e| ApiError::internal(format!("start_session: {e}")))?;
    let stored_medium = if medium == "pstn" {
        "voice".into()
    } else {
        medium
    };
    if stored_medium == "voice" {
        for text in state
            .store
            .live_sessions_with_medium(tenant_id, cid, "text")?
        {
            let _ = end_session(state, tenant_id, &text.id).await;
        }
    }
    let row = persist_session(state, tenant_id, cid, &sid, &stored_medium)?;
    join_conversation_participants(state, tenant_id, cid, &sid).await?;
    if stored_medium == "voice" {
        ensure_customer_connection(state, tenant_id, cid, &sid)?;
    }
    Ok(row)
}

pub fn persist_session(
    state: &AppState,
    tenant_id: &str,
    cid: &str,
    sid: &SessionId,
    medium: &str,
) -> Result<SessionRow> {
    let row = SessionRow {
        id: sid.to_string(),
        tenant_id: tenant_id.to_string(),
        conversation_id: cid.to_string(),
        medium: medium.to_string(),
        state: "active".into(),
        started_at: Utc::now().to_rfc3339(),
        ended_at: None,
    };
    state.store.insert_session(&row)?;
    state.store.touch_conversation(tenant_id, cid)?;
    crate::events::emit(
        state,
        tenant_id,
        Some(cid),
        "session.started",
        json!({ "sid": row.id, "medium": medium }),
    )?;
    if medium == "voice" {
        let _ = crate::recording::on_voice_session_started(state, tenant_id, cid, &row.id);
    }
    Ok(row)
}

async fn join_conversation_participants(
    state: &AppState,
    tenant_id: &str,
    cid: &str,
    sid: &SessionId,
) -> Result<()> {
    use rvoip_core::participant::{ParticipantKind, ParticipantRole};
    for p in state.store.list_participants(tenant_id, cid)? {
        let kind = match p.kind.as_str() {
            "ai" => ParticipantKind::Ai,
            "system" => ParticipantKind::System,
            "external" => ParticipantKind::External,
            _ => ParticipantKind::Human,
        };
        let role = match p.role.as_str() {
            "agent" => ParticipantRole::Agent,
            "supervisor" => ParticipantRole::Supervisor,
            "observer" => ParticipantRole::Observer,
            "customer" => ParticipantRole::Customer,
            other => ParticipantRole::Custom(other.into()),
        };
        let _ = state
            .orchestrator
            .join_session(sid.clone(), ParticipantId::from_string(p.id), kind, role)
            .await;
    }
    Ok(())
}

fn ensure_customer_connection(
    state: &AppState,
    tenant_id: &str,
    cid: &str,
    sid: &SessionId,
) -> Result<()> {
    use crate::store::ConnectionRow;
    let customer = state
        .store
        .list_participants(tenant_id, cid)?
        .into_iter()
        .find(|p| p.role == "customer")
        .ok_or_else(|| ApiError::not_found("customer participant"))?;
    let conn_id = format!("conn_customer_{sid}");
    if state.store.get_connection(tenant_id, &conn_id)?.is_some() {
        return Ok(());
    }
    state.store.insert_connection(&ConnectionRow {
        id: conn_id,
        tenant_id: tenant_id.into(),
        session_id: sid.to_string(),
        participant_id: customer.id,
        transport: "websocket".into(),
        state: "connected".into(),
    })?;
    Ok(())
}

pub async fn end_session(state: &AppState, tenant_id: &str, sid: &str) -> Result<SessionRow> {
    let row = state
        .store
        .get_session(tenant_id, sid)?
        .ok_or_else(|| ApiError::not_found("session not found"))?;
    let _ = state
        .orchestrator
        .end_session(
            SessionId::from_string(sid.to_string()),
            rvoip_core::EndReason::Normal,
        )
        .await;
    state.store.end_session(tenant_id, sid)?;
    crate::events::emit(
        state,
        tenant_id,
        Some(&row.conversation_id),
        "session.ended",
        json!({ "sid": sid }),
    )?;
    view_session(state, tenant_id, sid)
}

fn view_session(state: &AppState, tenant_id: &str, sid: &str) -> Result<SessionRow> {
    state
        .store
        .get_session(tenant_id, sid)?
        .ok_or_else(|| ApiError::not_found("session not found"))
}

#[derive(Clone, Debug, Serialize)]
pub struct Timeline {
    pub items: Vec<TimelineItem>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type")]
pub enum TimelineItem {
    #[serde(rename = "session")]
    Session {
        id: String,
        medium: String,
        state: String,
        started_at: String,
        ended_at: Option<String>,
    },
    #[serde(rename = "message")]
    Message {
        id: String,
        medium: String,
        body: String,
        from_participant: Option<String>,
        created_at: String,
    },
}

pub fn timeline(state: &AppState, tenant_id: &str, cid: &str) -> Result<Timeline> {
    let _ = state
        .store
        .get_conversation(tenant_id, cid)?
        .ok_or_else(|| ApiError::not_found("conversation not found").with_conversation(cid))?;
    let mut items: Vec<(String, TimelineItem)> = Vec::new();
    for s in state.store.list_sessions(tenant_id, cid)? {
        items.push((
            s.started_at.clone(),
            TimelineItem::Session {
                id: s.id,
                medium: s.medium,
                state: s.state,
                started_at: s.started_at.clone(),
                ended_at: s.ended_at,
            },
        ));
    }
    for m in state.store.list_messages(tenant_id, cid)? {
        items.push((
            m.created_at.clone(),
            TimelineItem::Message {
                id: m.id,
                medium: m.medium,
                body: m.body,
                from_participant: m.from_participant,
                created_at: m.created_at.clone(),
            },
        ));
    }
    items.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(Timeline {
        items: items.into_iter().map(|(_, i)| i).collect(),
    })
}

pub fn hours_open(config: &crate::config::Config) -> bool {
    crate::hours::is_open(config)
}

/// After-hours PSTN: persist a voicemail Message. No live voice Session, no AI pitch.
pub fn leave_voicemail(state: &AppState, tenant_id: &str, cid: &str) -> Result<MessageRow> {
    let row = MessageRow {
        id: format!("msg_{}", Uuid::new_v4().simple()),
        tenant_id: tenant_id.to_string(),
        conversation_id: cid.to_string(),
        from_participant: None,
        medium: "audio".into(),
        body: "Voicemail".into(),
        provider_id: None,
        state: "accepted".into(),
        created_at: Utc::now().to_rfc3339(),
    };
    state.store.insert_message(&row)?;
    state.store.touch_conversation(tenant_id, cid)?;
    crate::events::emit(
        state,
        tenant_id,
        Some(cid),
        "message.received",
        json!({ "id": row.id, "medium": "audio", "kind": "voicemail" }),
    )?;
    Ok(row)
}
