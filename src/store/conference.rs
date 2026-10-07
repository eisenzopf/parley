//! Durable membership, recipient routing, command outcomes and SMS outbox.
use super::{MessageRow, Store};
use crate::{ApiError, Result};
use chrono::{Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemberInput {
    pub alias: String,
    pub name: String,
    pub role: String,
    #[serde(default)]
    pub sms: Option<String>,
    #[serde(default)]
    pub sip: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Member {
    pub participant_id: String,
    pub subject: String,
    pub alias: String,
    pub name: String,
    pub role: String,
    pub sms: Option<String>,
    pub sip: Option<String>,
}

impl Member {
    pub fn observes_all(&self) -> bool {
        matches!(self.role.as_str(), "owner" | "assistant")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JournalEvent {
    pub seq: i64,
    pub cid: String,
    pub event_type: String,
    pub request_id: Option<String>,
    pub payload: Value,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Delivery {
    pub id: String,
    pub tenant_id: String,
    pub conversation_id: String,
    pub message_id: String,
    pub participant_id: String,
    pub sender_address: String,
    pub recipient_address: String,
    pub provider_id: Option<String>,
    pub state: String,
    pub error: Option<String>,
    pub body: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VoiceOperation {
    pub conversation_id: String,
    pub session_id: String,
    pub request_id: String,
    pub target_participant_id: String,
    pub assistant_participant_id: String,
    pub remote_connection_id: String,
    pub purpose: String,
    pub ai_state: String,
    pub ai_connection_id: Option<String>,
    pub bridge_id: Option<String>,
}

#[derive(Serialize)]
pub struct HistoryPage {
    pub messages: Vec<MessageRow>,
    pub cursor: i64,
    pub has_more: bool,
}

#[derive(Clone, Debug)]
pub enum InboundSmsOutcome {
    Unmatched,
    Routed(MessageRow),
    Held { id: i64 },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SmsRoute {
    pub conversation_id: String,
    pub participant_id: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct HeldSms {
    pub id: i64,
    pub provider_id: String,
    pub remote_address: String,
    pub local_address: String,
    pub body: String,
    pub candidates: Vec<SmsRoute>,
    pub received_at: String,
}

pub(super) fn append_event(
    conn: &Connection,
    tenant: &str,
    cid: &str,
    kind: &str,
    request: Option<&str>,
    audience: &[String],
    payload: Value,
) -> Result<()> {
    conn.execute("INSERT INTO conference_events(tenant_id,conversation_id,event_type,request_id,audience,payload,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![tenant,cid,kind,request,json!(audience).to_string(),payload.to_string(),Utc::now().to_rfc3339()])?;
    Ok(())
}

pub fn validate_members(inputs: &[MemberInput]) -> Result<()> {
    let mut aliases = std::collections::HashSet::new();
    if inputs.is_empty() || inputs.len() > 32 {
        return Err(ApiError::bad_request("one to 32 participants required"));
    }
    for p in inputs {
        if p.alias.is_empty()
            || p.alias.len() > 64
            || !p
                .alias
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            || !aliases.insert(&p.alias)
        {
            return Err(ApiError::bad_request(
                "participant aliases must be unique identifiers",
            ));
        }
        if p.name.trim().is_empty()
            || p.name.len() > 160
            || !matches!(
                p.role.as_str(),
                "owner" | "assistant" | "companion" | "booker" | "organizer" | "participant"
            )
        {
            return Err(ApiError::bad_request("invalid participant name or role"));
        }
        if let Some(phone) = &p.sms {
            if !phone.starts_with('+')
                || !(9..=16).contains(&phone.len())
                || !phone[1..].bytes().all(|b| b.is_ascii_digit())
            {
                return Err(ApiError::bad_request("SMS endpoint must be E.164"));
            }
        }
        if p.sip
            .as_ref()
            .is_some_and(|s| !(s.starts_with("sip:") || s.starts_with("sips:")) || s.len() > 512)
        {
            return Err(ApiError::bad_request("invalid SIP endpoint"));
        }
    }
    Ok(())
}

impl Store {
    pub fn record_conference_receipt(
        &self,
        tenant: &str,
        provider_id: &str,
        state: &str,
        error: Option<&str>,
    ) -> Result<()> {
        if !matches!(state, "sent" | "delivered" | "failed") {
            return Err(ApiError::bad_request("invalid provider receipt"));
        }
        self.conn.lock().expect("store lock").execute("INSERT INTO conference_provider_receipts(tenant_id,provider_id,state,error) VALUES(?1,?2,?3,?4) ON CONFLICT(tenant_id,provider_id) DO UPDATE SET state=excluded.state,error=excluded.error WHERE conference_provider_receipts.state NOT IN ('delivered','failed')",params![tenant,provider_id,state,error])?;
        Ok(())
    }
    pub fn conference_voice_for_session(
        &self,
        tenant: &str,
        sid: &str,
    ) -> Result<Option<VoiceOperation>> {
        let remote:Option<String>=self.conn.lock().expect("store lock").query_row("SELECT remote_connection_id FROM conference_voice_operations WHERE tenant_id=?1 AND session_id=?2",params![tenant,sid],|r|r.get(0)).optional()?;
        match remote {
            Some(remote) => self.conference_voice_for_connection(tenant, &remote),
            None => Ok(None),
        }
    }

    pub fn prepare_conference_browser(
        &self,
        tenant: &str,
        voice: &VoiceOperation,
        pid: &str,
        connid: &str,
        request: &str,
    ) -> Result<()> {
        let mut conn = self.conn.lock().expect("store lock");
        let tx = conn.transaction()?;
        let exists:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM conference_browser_connections WHERE tenant_id=?1 AND session_id=?2 AND state IN ('offered','answered','speaking'))",params![tenant,voice.session_id],|r|r.get(0))?;
        if exists {
            return Err(ApiError::conflict(
                "a browser connection already exists for this Session",
            ));
        }
        tx.execute("INSERT INTO connections(id,tenant_id,session_id,participant_id,transport,state) VALUES(?1,?2,?3,?4,'webrtc','connecting')",params![connid,tenant,voice.session_id,pid])?;
        tx.execute("INSERT INTO conference_browser_connections(connection_id,tenant_id,conversation_id,session_id,participant_id,request_id) VALUES(?1,?2,?3,?4,?5,?6)",params![connid,tenant,voice.conversation_id,voice.session_id,pid,request])?;
        append_event(
            &tx,
            tenant,
            &voice.conversation_id,
            "connection.offered",
            Some(request),
            &[pid.into()],
            json!({"sid":voice.session_id,"connid":connid,"participant_id":pid,"transport":"webrtc","state":"connecting"}),
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn require_conference_browser(
        &self,
        tenant: &str,
        cid: &str,
        sid: &str,
        pid: &str,
        connid: &str,
    ) -> Result<()> {
        let allowed:bool=self.conn.lock().expect("store lock").query_row("SELECT EXISTS(SELECT 1 FROM conference_browser_connections WHERE tenant_id=?1 AND conversation_id=?2 AND session_id=?3 AND participant_id=?4 AND connection_id=?5 AND state IN ('offered','answered','speaking'))",params![tenant,cid,sid,pid,connid],|r|r.get(0))?;
        if allowed {
            Ok(())
        } else {
            Err(ApiError::forbidden(
                "browser connection is not owned by this participant in this Session",
            ))
        }
    }

    pub fn update_conference_browser(
        &self,
        tenant: &str,
        connid: &str,
        state: &str,
        request: Option<&str>,
        details: Value,
    ) -> Result<()> {
        let mut conn = self.conn.lock().expect("store lock");
        let tx = conn.transaction()?;
        let found:Option<(String,String,String,String,String)>=tx.query_row("SELECT conversation_id,session_id,participant_id,request_id,state FROM conference_browser_connections WHERE tenant_id=?1 AND connection_id=?2",params![tenant,connid],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
        let Some((cid, sid, pid, join, previous)) = found else {
            return Ok(());
        };
        if previous == "retired" && matches!(state, "ended" | "failed") {
            tx.execute(
                "UPDATE connections SET state=?3 WHERE tenant_id=?1 AND id=?2",
                params![tenant, connid, state],
            )?;
            tx.commit()?;
            return Ok(());
        }
        if matches!(previous.as_str(), "failed" | "ended" | "retired") || previous == state {
            return Ok(());
        }
        tx.execute("UPDATE conference_browser_connections SET state=?3 WHERE tenant_id=?1 AND connection_id=?2",params![tenant,connid,state])?;
        if state == "speaking" {
            let bridge = details["bridge_id"]
                .as_str()
                .ok_or_else(|| ApiError::internal("committed browser bridge missing"))?;
            tx.execute("INSERT INTO conference_speaking_routes(session_id,tenant_id,connection_id,participant_id,bridge_id) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(session_id) DO UPDATE SET connection_id=excluded.connection_id,participant_id=excluded.participant_id,bridge_id=excluded.bridge_id",params![sid,tenant,connid,pid,bridge])?;
        }
        let connection_state = if state == "speaking" {
            "connected"
        } else {
            state
        };
        tx.execute(
            "UPDATE connections SET state=?3 WHERE tenant_id=?1 AND id=?2",
            params![tenant, connid, connection_state],
        )?;
        append_event(
            &tx,
            tenant,
            &cid,
            &format!("browser.{state}"),
            Some(request.unwrap_or(&join)),
            &[pid],
            json!({"sid":sid,"connid":connid,"state":state,"details":details}),
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn require_answered_conference_browser(&self, tenant: &str, connid: &str) -> Result<()> {
        let answered: bool = self.conn.lock().expect("store lock").query_row(
            "SELECT EXISTS(SELECT 1 FROM conference_browser_connections WHERE tenant_id=?1 AND connection_id=?2 AND state='answered')",
            params![tenant, connid], |row| row.get(0),
        )?;
        if answered {
            Ok(())
        } else {
            Err(ApiError::conflict(
                "browser media answer required before handoff",
            ))
        }
    }

    pub fn prepare_conference_voice(
        &self,
        tenant: &str,
        cid: &str,
        sid: &str,
        request: &str,
        target: &Member,
        assistant: &Member,
        connid: &str,
        purpose: &str,
        initiated_by: &str,
    ) -> Result<()> {
        let mut conn = self.conn.lock().expect("store lock");
        let tx = conn.transaction()?;
        let open: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM conversations WHERE tenant_id=?1 AND id=?2 AND state='open')", params![tenant,cid], |r| r.get(0))?;
        if !open {
            return Err(ApiError::conflict("Conversation is closed"));
        }
        let now = Utc::now().to_rfc3339();
        tx.execute("INSERT INTO sessions(id,tenant_id,conversation_id,medium,state,started_at) VALUES(?1,?2,?3,'voice','active',?4)",params![sid,tenant,cid,now])?;
        tx.execute("INSERT INTO connections(id,tenant_id,session_id,participant_id,transport,state) VALUES(?1,?2,?3,?4,'sip','prepared')",params![connid,tenant,sid,target.participant_id])?;
        tx.execute("INSERT INTO conference_voice_operations(tenant_id,conversation_id,session_id,request_id,target_participant_id,assistant_participant_id,remote_connection_id,purpose) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![tenant,cid,sid,request,target.participant_id,assistant.participant_id,connid,purpose])?;
        append_event(
            &tx,
            tenant,
            cid,
            "session.invited",
            Some(request),
            &[
                target.participant_id.clone(),
                assistant.participant_id.clone(),
            ],
            json!({"sid":sid,"connid":connid,"participant_id":target.participant_id,"purpose":purpose,"initiated_by":initiated_by,"state":"accepted","transport":"sip"}),
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn conference_voice_for_connection(
        &self,
        tenant: &str,
        connid: &str,
    ) -> Result<Option<VoiceOperation>> {
        self.conn.lock().expect("store lock").query_row("SELECT conversation_id,session_id,request_id,target_participant_id,assistant_participant_id,remote_connection_id,purpose,ai_state,ai_connection_id,bridge_id FROM conference_voice_operations WHERE tenant_id=?1 AND remote_connection_id=?2",params![tenant,connid],|r|Ok(VoiceOperation{conversation_id:r.get(0)?,session_id:r.get(1)?,request_id:r.get(2)?,target_participant_id:r.get(3)?,assistant_participant_id:r.get(4)?,remote_connection_id:r.get(5)?,purpose:r.get(6)?,ai_state:r.get(7)?,ai_connection_id:r.get(8)?,bridge_id:r.get(9)?})).optional().map_err(Into::into)
    }

    pub fn conference_voice_connection_event(
        &self,
        tenant: &str,
        connid: &str,
        state: &str,
        details: Value,
    ) -> Result<()> {
        let Some(voice) = self.conference_voice_for_connection(tenant, connid)? else {
            return Ok(());
        };
        let mut conn = self.conn.lock().expect("store lock");
        let tx = conn.transaction()?;
        let prior: String = tx.query_row(
            "SELECT state FROM connections WHERE tenant_id=?1 AND id=?2",
            params![tenant, connid],
            |r| r.get(0),
        )?;
        if matches!(prior.as_str(), "ended" | "failed") || prior == state {
            return Ok(());
        }
        if state != "progress" {
            if state == "dialing" && prior == "connected" {
                return Ok(());
            }
            tx.execute(
                "UPDATE connections SET state=?3 WHERE tenant_id=?1 AND id=?2",
                params![tenant, connid, state],
            )?;
        }
        append_event(
            &tx,
            tenant,
            &voice.conversation_id,
            &format!("connection.{state}"),
            Some(&voice.request_id),
            &[
                voice.target_participant_id.clone(),
                voice.assistant_participant_id.clone(),
            ],
            json!({"sid":voice.session_id,"connid":connid,"participant_id":voice.target_participant_id,"state":state,"transport":"sip","details":details}),
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn finish_conference_voice(
        &self,
        tenant: &str,
        sid: &str,
        state: &str,
        request: Option<&str>,
    ) -> Result<()> {
        let mut conn = self.conn.lock().expect("store lock");
        let tx = conn.transaction()?;
        let found:Option<(String,String,String)>=tx.query_row("SELECT v.conversation_id,v.request_id,s.state FROM conference_voice_operations v JOIN sessions s ON s.id=v.session_id WHERE v.tenant_id=?1 AND v.session_id=?2",params![tenant,sid],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        let Some((cid, invite, prior)) = found else {
            return Ok(());
        };
        if matches!(prior.as_str(), "ended" | "failed") {
            return Ok(());
        }
        tx.execute(
            "UPDATE sessions SET state=?3,ended_at=?4 WHERE tenant_id=?1 AND id=?2",
            params![tenant, sid, state, Utc::now().to_rfc3339()],
        )?;
        tx.execute("UPDATE connections SET state='ended' WHERE tenant_id=?1 AND session_id=?2 AND state!='failed'",params![tenant,sid])?;
        tx.execute("UPDATE conference_phone_moves SET state='ended' WHERE tenant_id=?1 AND session_id=?2 AND state NOT IN ('failed','cancelled','ended')",params![tenant,sid])?;
        append_event(
            &tx,
            tenant,
            &cid,
            &format!("session.{state}"),
            Some(request.unwrap_or(&invite)),
            &[],
            json!({"sid":sid,"state":state}),
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn claim_conference_ai(&self, tenant: &str, sid: &str) -> Result<bool> {
        Ok(self.conn.lock().expect("store lock").execute("UPDATE conference_voice_operations SET ai_state='attaching' WHERE tenant_id=?1 AND session_id=?2 AND ai_state='waiting' AND EXISTS(SELECT 1 FROM sessions WHERE id=?2 AND state='active')",params![tenant,sid])?==1)
    }

    pub fn conference_ai_attached(
        &self,
        tenant: &str,
        voice: &VoiceOperation,
        ai_conn: &str,
        bridge: &str,
    ) -> Result<()> {
        let mut conn = self.conn.lock().expect("store lock");
        let tx = conn.transaction()?;
        tx.execute("UPDATE conference_voice_operations SET ai_state='attached',ai_connection_id=?3,bridge_id=?4 WHERE tenant_id=?1 AND session_id=?2",params![tenant,voice.session_id,ai_conn,bridge])?;
        tx.execute("INSERT INTO conference_speaking_routes(session_id,tenant_id,connection_id,participant_id,bridge_id) VALUES(?1,?2,?3,?4,?5)",params![voice.session_id,tenant,ai_conn,voice.assistant_participant_id,bridge])?;
        tx.execute("INSERT INTO connections(id,tenant_id,session_id,participant_id,transport,state) VALUES(?1,?2,?3,?4,'vapi','connected')",params![ai_conn,tenant,voice.session_id,voice.assistant_participant_id])?;
        append_event(
            &tx,
            tenant,
            &voice.conversation_id,
            "session.assistant_attached",
            Some(&voice.request_id),
            &[
                voice.target_participant_id.clone(),
                voice.assistant_participant_id.clone(),
            ],
            json!({"sid":voice.session_id,"connid":ai_conn,"remote_connid":voice.remote_connection_id,"participant_id":voice.assistant_participant_id,"bridge_id":bridge,"transport":"vapi","state":"connected"}),
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn conference_voice_fact(
        &self,
        tenant: &str,
        voice: &VoiceOperation,
        kind: &str,
        payload: Value,
    ) -> Result<()> {
        append_event(
            &self.conn.lock().expect("store lock"),
            tenant,
            &voice.conversation_id,
            kind,
            Some(&voice.request_id),
            &[
                voice.target_participant_id.clone(),
                voice.assistant_participant_id.clone(),
            ],
            payload,
        )
    }

    /// Record deliberate shutdown before ending transports. Provider writes may
    /// fail while the socket is closing; those are not unexpected call failures.
    /// Never overwrite a failure that was already observed or a human's route.
    pub fn mark_conference_ai_ending(&self, tenant: &str, sid: &str) -> Result<bool> {
        let changed = self.conn.lock().expect("store lock").execute(
            "UPDATE conference_voice_operations SET ai_state='ending'
             WHERE tenant_id=?1 AND session_id=?2 AND ai_state='attached'
             AND EXISTS (SELECT 1 FROM conference_speaking_routes r
                 WHERE r.tenant_id=?1 AND r.session_id=?2
                 AND r.connection_id=conference_voice_operations.ai_connection_id)",
            params![tenant, sid],
        )?;
        Ok(changed == 1)
    }

    /// Preserve a failed AI transport in the journal even when paired teardown
    /// also emits SessionEnded. A retired AI leg cannot fail a human's new route.
    pub fn record_conference_ai_failure(&self, tenant: &str, connid: &str) -> Result<bool> {
        let mut conn = self.conn.lock().expect("store lock");
        let tx = conn.transaction()?;
        let found: Option<(String, String, String, String, String)> = tx.query_row(
            "SELECT v.conversation_id,v.session_id,v.request_id,v.target_participant_id,v.assistant_participant_id
             FROM conference_voice_operations v JOIN conference_speaking_routes r
             ON r.tenant_id=v.tenant_id AND r.session_id=v.session_id
             WHERE v.tenant_id=?1 AND v.ai_connection_id=?2 AND v.ai_state='attached' AND r.connection_id=?2",
            params![tenant, connid],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        ).optional()?;
        let Some((cid, sid, request, target, assistant)) = found else { return Ok(false); };
        tx.execute("UPDATE conference_voice_operations SET ai_state='failed' WHERE tenant_id=?1 AND session_id=?2", params![tenant, sid])?;
        tx.execute("UPDATE connections SET state='failed' WHERE tenant_id=?1 AND id=?2", params![tenant, connid])?;
        append_event(&tx, tenant, &cid, "session.assistant_failed", Some(&request), &[target, assistant],
            json!({"sid":sid,"connid":connid,"state":"failed","reason":"AI voice transport failed"}))?;
        tx.commit()?;
        Ok(true)
    }

    /// Claim one delivery transactionally. An interrupted submission is never
    /// automatically resent: its provider outcome must first be reconciled.
    pub fn claim_conference_delivery(&self) -> Result<Option<Delivery>> {
        let mut conn = self.conn.lock().expect("store lock");
        let tx = conn.transaction()?;
        let delivery=tx.query_row("SELECT d.id,d.tenant_id,d.conversation_id,d.message_id,d.participant_id,d.sender_address,d.recipient_address,d.provider_id,d.state,d.error,m.body FROM message_deliveries d JOIN messages m ON m.id=d.message_id WHERE d.state='queued' ORDER BY d.created_at LIMIT 1",[],map_delivery).optional()?;
        if let Some(d) = &delivery {
            tx.execute("UPDATE message_deliveries SET state='submitting',updated_at=?2 WHERE id=?1 AND state='queued'",params![d.id,Utc::now().to_rfc3339()])?;
        }
        tx.commit()?;
        Ok(delivery)
    }

    pub fn recover_conference_outbox(&self) -> Result<()> {
        let interrupted: Vec<(String, String, Option<String>)> = {
            let conn = self.conn.lock().expect("store lock");
            let mut stmt = conn.prepare(
                "SELECT tenant_id,id,provider_id FROM message_deliveries WHERE state='submitting'",
            )?;
            let rows = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<std::result::Result<_, _>>()?;
            rows
        };
        for (tenant, id, provider) in interrupted {
            self.update_conference_delivery(
                &tenant,
                &id,
                provider.as_deref(),
                "unknown",
                Some("interrupted submission; reconcile before retry"),
            )?;
        }
        Ok(())
    }

    /// A new process has no authority to claim a remote telephone call ended.
    /// Preserve the IDs and block redial until the owner verifies termination.
    pub fn recover_conference_voice(&self) -> Result<()> {
        let mut conn = self.conn.lock().expect("store lock");
        let tx = conn.transaction()?;
        let interrupted: Vec<(String, String, String, String, String, String)> = {
            let mut stmt=tx.prepare("SELECT v.tenant_id,v.conversation_id,v.session_id,v.request_id,v.target_participant_id,v.assistant_participant_id FROM conference_voice_operations v JOIN sessions s ON s.id=v.session_id WHERE s.state NOT IN ('ended','failed','interrupted')")?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                })?
                .collect::<std::result::Result<_, _>>()?;
            rows
        };
        for (tenant, cid, sid, request, target, assistant) in interrupted {
            tx.execute(
                "UPDATE sessions SET state='interrupted' WHERE tenant_id=?1 AND id=?2",
                params![tenant, sid],
            )?;
            tx.execute("UPDATE connections SET state='unknown' WHERE tenant_id=?1 AND session_id=?2 AND state NOT IN ('ended','failed')",params![tenant,sid])?;
            tx.execute("UPDATE conference_browser_connections SET state='failed' WHERE tenant_id=?1 AND session_id=?2 AND state NOT IN ('ended','failed')",params![tenant,sid])?;
            tx.execute("UPDATE conference_voice_operations SET ai_state='interrupted' WHERE tenant_id=?1 AND session_id=?2",params![tenant,sid])?;
            tx.execute("UPDATE conference_phone_moves SET state='interrupted' WHERE tenant_id=?1 AND session_id=?2 AND state NOT IN ('ended','failed','cancelled')",params![tenant,sid])?;
            append_event(
                &tx,
                &tenant,
                &cid,
                "session.interrupted",
                Some(&request),
                &[target, assistant],
                json!({"sid":sid,"state":"interrupted","reason":"server restarted; remote call termination is unknown","requires_owner_verification":true}),
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn confirm_interrupted_voice_ended(
        &self,
        tenant: &str,
        cid: &str,
        sid: &str,
        actor: &Member,
        note: &str,
        request: &str,
    ) -> Result<Value> {
        if actor.role != "owner" {
            return Err(ApiError::forbidden(
                "only the owner may attest remote call termination",
            ));
        }
        if note.trim().is_empty() || note.len() > 2000 {
            return Err(ApiError::bad_request(
                "termination verification note required, maximum 2000 bytes",
            ));
        }
        let mut conn = self.conn.lock().expect("store lock");
        let tx = conn.transaction()?;
        let participants:Option<(String,String)>=tx.query_row("SELECT v.target_participant_id,v.assistant_participant_id FROM conference_voice_operations v JOIN sessions s ON s.id=v.session_id WHERE v.tenant_id=?1 AND v.conversation_id=?2 AND v.session_id=?3 AND s.state='interrupted'",params![tenant,cid,sid],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        let Some((target, assistant)) = participants else {
            return Err(ApiError::conflict(
                "only an interrupted Session can be reconciled",
            ));
        };
        tx.execute(
            "UPDATE sessions SET state='ended',ended_at=?3 WHERE tenant_id=?1 AND id=?2",
            params![tenant, sid, Utc::now().to_rfc3339()],
        )?;
        tx.execute("UPDATE connections SET state='ended' WHERE tenant_id=?1 AND session_id=?2 AND state='unknown'",params![tenant,sid])?;
        let fact = json!({"sid":sid,"state":"ended","source":"owner_verification","verified_by":actor.participant_id,"note":note});
        append_event(
            &tx,
            tenant,
            cid,
            "session.ended",
            Some(request),
            &[target, assistant],
            fact.clone(),
        )?;
        tx.commit()?;
        Ok(fact)
    }

    pub fn conference_deliveries(&self, tenant: &str, cid: &str) -> Result<Vec<Delivery>> {
        let conn = self.conn.lock().expect("store lock");
        let mut stmt=conn.prepare("SELECT d.id,d.tenant_id,d.conversation_id,d.message_id,d.participant_id,d.sender_address,d.recipient_address,d.provider_id,d.state,d.error,m.body FROM message_deliveries d JOIN messages m ON m.id=d.message_id WHERE d.tenant_id=?1 AND d.conversation_id=?2 ORDER BY d.created_at,d.rowid")?;
        let rows = stmt
            .query_map(params![tenant, cid], map_delivery)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn conference_delivery_for_provider(
        &self,
        tenant: &str,
        provider_id: &str,
    ) -> Result<Option<Delivery>> {
        self.conn.lock().expect("store lock").query_row("SELECT d.id,d.tenant_id,d.conversation_id,d.message_id,d.participant_id,d.sender_address,d.recipient_address,d.provider_id,d.state,d.error,m.body FROM message_deliveries d JOIN messages m ON m.id=d.message_id WHERE d.tenant_id=?1 AND d.provider_id=?2",params![tenant,provider_id],map_delivery).optional().map_err(Into::into)
    }

    pub fn update_conference_delivery(
        &self,
        tenant: &str,
        id: &str,
        provider_id: Option<&str>,
        state: &str,
        error: Option<&str>,
    ) -> Result<()> {
        if !matches!(state, "sent" | "delivered" | "failed" | "unknown") {
            return Err(ApiError::bad_request("invalid delivery transition"));
        }
        let mut conn = self.conn.lock().expect("store lock");
        let tx = conn.transaction()?;
        let (cid,mid,pid,previous):(String,String,String,String)=tx.query_row("SELECT conversation_id,message_id,participant_id,state FROM message_deliveries WHERE tenant_id=?1 AND id=?2",params![tenant,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
        let receipt: Option<(String, Option<String>)> = if let Some(provider) = provider_id {
            tx.query_row("SELECT state,error FROM conference_provider_receipts WHERE tenant_id=?1 AND provider_id=?2 AND state IN ('delivered','failed')",params![tenant,provider],|r|Ok((r.get(0)?,r.get(1)?))).optional()?
        } else {
            None
        };
        let (state, error) = receipt
            .as_ref()
            .map(|(s, e)| (s.as_str(), e.as_deref()))
            .unwrap_or((state, error));
        // Final provider states are monotonic; duplicate or late callbacks do
        // not rewrite a completed result or emit duplicate UI transitions.
        if previous == state || matches!(previous.as_str(), "delivered" | "failed") {
            return Ok(());
        }
        tx.execute("UPDATE message_deliveries SET provider_id=COALESCE(?3,provider_id),state=?4,error=?5,updated_at=?6 WHERE tenant_id=?1 AND id=?2",params![tenant,id,provider_id,state,error,Utc::now().to_rfc3339()])?;
        let request: String = tx.query_row(
            "SELECT request_id FROM message_metadata WHERE message_id=?1",
            params![mid],
            |r| r.get(0),
        )?;
        let actor: Option<String> = tx.query_row(
            "SELECT from_participant FROM messages WHERE id=?1",
            params![mid],
            |r| r.get(0),
        )?;
        let mut audience = vec![pid.clone()];
        if let Some(actor) = actor {
            audience.push(actor);
        }
        append_event(
            &tx,
            tenant,
            &cid,
            "message.delivery",
            Some(&request),
            &audience,
            json!({"delivery_id":id,"msg_id":mid,"participant_id":pid,"state":state,"error":error}),
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Number-pair matches are authoritative only when exactly one route exists.
    /// Ambiguous receipts are acknowledged after durable holding, never guessed.
    pub fn receive_conference_sms(
        &self,
        tenant: &str,
        provider_id: &str,
        remote: &str,
        local: &str,
        body: &str,
    ) -> Result<InboundSmsOutcome> {
        if provider_id.is_empty()
            || provider_id.len() > 512
            || body.is_empty()
            || body.len() > 16000
            || remote.len() > 64
            || local.len() > 64
        {
            return Err(ApiError::bad_request("invalid inbound SMS fields"));
        }
        let mut conn = self.conn.lock().expect("store lock");
        let tx = conn.transaction()?;
        let held:Option<(i64,String,String,String,Option<String>)>=tx.query_row(
            "SELECT id,remote_address,local_address,body,message_id FROM conference_held_sms WHERE tenant_id=?1 AND provider_id=?2",
            params![tenant,provider_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
        if let Some((id, original_remote, original_local, original_body, mid)) = held {
            if original_remote != remote || original_local != local || original_body != body {
                return Err(ApiError::conflict(
                    "provider message ID reused with different content",
                ));
            }
            if let Some(mid) = mid {
                drop(tx);
                drop(conn);
                return self
                    .get_message(tenant, &mid)?
                    .map(InboundSmsOutcome::Routed)
                    .ok_or_else(|| ApiError::internal("resolved SMS message missing"));
            }
            return Ok(InboundSmsOutcome::Held { id });
        }
        let known:Option<String>=tx.query_row("SELECT message_id FROM conference_inbound_sms WHERE tenant_id=?1 AND provider_id=?2",params![tenant,provider_id],|r|r.get(0)).optional()?;
        if let Some(mid) = known {
            drop(tx);
            drop(conn);
            let row = self
                .get_message(tenant, &mid)?
                .ok_or_else(|| ApiError::internal("inbound SMS message missing"))?;
            if row.body != body {
                return Err(ApiError::conflict(
                    "provider message ID reused with different content",
                ));
            }
            return Ok(InboundSmsOutcome::Routed(row));
        }
        let candidates: Vec<SmsRoute> = {
            let mut stmt=tx.prepare("SELECT DISTINCT d.conversation_id,d.participant_id FROM message_deliveries d JOIN conversations c ON c.id=d.conversation_id AND c.tenant_id=d.tenant_id WHERE d.tenant_id=?1 AND d.recipient_address=?2 AND d.sender_address=?3 AND c.state='open' AND d.state IN ('submitting','sent','delivered','unknown') ORDER BY d.conversation_id,d.participant_id")?;
            let rows = stmt
                .query_map(params![tenant, remote, local], |r| {
                    Ok(SmsRoute {
                        conversation_id: r.get(0)?,
                        participant_id: r.get(1)?,
                    })
                })?
                .collect::<std::result::Result<_, _>>()?;
            rows
        };
        if candidates.is_empty() {
            return Ok(InboundSmsOutcome::Unmatched);
        }
        if candidates.len() != 1 {
            tx.execute("INSERT INTO conference_held_sms(tenant_id,provider_id,remote_address,local_address,body,candidates,received_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![tenant,provider_id,remote,local,body,serde_json::to_string(&candidates).map_err(|_|ApiError::internal("SMS routes encoding failed"))?,Utc::now().to_rfc3339()])?;
            let id = tx.last_insert_rowid();
            tx.commit()?;
            return Ok(InboundSmsOutcome::Held { id });
        }
        let route = &candidates[0];
        let row = persist_inbound_sms(
            &tx,
            tenant,
            provider_id,
            body,
            &route.conversation_id,
            &route.participant_id,
            None,
            Value::Null,
        )?;
        tx.commit()?;
        Ok(InboundSmsOutcome::Routed(row))
    }

    /// Administration projection only: never expose held text to candidate members.
    pub fn held_conference_sms(&self, tenant: &str, after: i64) -> Result<Vec<HeldSms>> {
        if after < 0 {
            return Err(ApiError::bad_request("invalid inbox cursor"));
        }
        let conn = self.conn.lock().expect("store lock");
        let mut stmt=conn.prepare("SELECT id,provider_id,remote_address,local_address,body,candidates,received_at FROM conference_held_sms WHERE tenant_id=?1 AND state='held' AND id>?2 ORDER BY id LIMIT 100")?;
        let rows = stmt
            .query_map(params![tenant, after], |r| {
                let raw: String = r.get(5)?;
                let candidates = serde_json::from_str(&raw).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        5,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                Ok(HeldSms {
                    id: r.get(0)?,
                    provider_id: r.get(1)?,
                    remote_address: r.get(2)?,
                    local_address: r.get(3)?,
                    body: r.get(4)?,
                    candidates,
                    received_at: r.get(6)?,
                })
            })?
            .collect::<std::result::Result<_, _>>()?;
        Ok(rows)
    }

    pub fn resolve_conference_sms(
        &self,
        tenant: &str,
        id: i64,
        cid: &str,
        pid: &str,
        note: &str,
        actor: &str,
        request: &str,
    ) -> Result<MessageRow> {
        if actor != "api" {
            return Err(ApiError::forbidden("SMS routing requires administrator"));
        }
        if note.trim().is_empty() || note.len() > 2000 {
            return Err(ApiError::bad_request(
                "routing verification note required, maximum 2000 bytes",
            ));
        }
        let mut conn = self.conn.lock().expect("store lock");
        let tx = conn.transaction()?;
        let entry:Option<(String,String,String,String,Option<String>,Option<String>,Option<String>,Option<String>)>=tx.query_row(
            "SELECT provider_id,remote_address,body,candidates,message_id,resolved_conversation_id,resolved_participant_id,resolution_note FROM conference_held_sms WHERE tenant_id=?1 AND id=?2",params![tenant,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?))).optional()?;
        let Some((provider, remote, body, raw, mid, prior_cid, prior_pid, prior_note)) = entry
        else {
            return Err(ApiError::not_found("held SMS not found"));
        };
        if let Some(mid) = mid {
            if prior_cid.as_deref() != Some(cid)
                || prior_pid.as_deref() != Some(pid)
                || prior_note.as_deref() != Some(note)
            {
                return Err(ApiError::conflict("SMS already resolved differently"));
            }
            drop(tx);
            drop(conn);
            return self
                .get_message(tenant, &mid)?
                .ok_or_else(|| ApiError::internal("resolved SMS message missing"));
        }
        let candidates: Vec<SmsRoute> = serde_json::from_str(&raw)
            .map_err(|_| ApiError::internal("stored SMS routes invalid"))?;
        if !candidates
            .iter()
            .any(|r| r.conversation_id == cid && r.participant_id == pid)
        {
            return Err(ApiError::bad_request(
                "resolution must name an original candidate",
            ));
        }
        let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM conference_members m JOIN conversations c ON c.id=m.conversation_id AND c.tenant_id=m.tenant_id JOIN participant_endpoints e ON e.participant_id=m.participant_id AND e.tenant_id=m.tenant_id WHERE m.tenant_id=?1 AND m.conversation_id=?2 AND m.participant_id=?3 AND c.state='open' AND e.kind='sms' AND e.address=?4)",params![tenant,cid,pid,remote],|r|r.get(0))?;
        if !valid {
            return Err(ApiError::conflict(
                "candidate Conversation or participant endpoint is no longer available",
            ));
        }
        let row = persist_inbound_sms(
            &tx,
            tenant,
            &provider,
            &body,
            cid,
            pid,
            Some(request),
            json!({"source":"administrator_resolution","inbox_id":id,"resolved_by":actor,"note":note}),
        )?;
        tx.execute("UPDATE conference_held_sms SET state='resolved',message_id=?3,resolved_conversation_id=?4,resolved_participant_id=?5,resolved_by=?6,resolution_note=?7,resolved_at=?8 WHERE tenant_id=?1 AND id=?2",params![tenant,id,row.id,cid,pid,actor,note,Utc::now().to_rfc3339()])?;
        tx.commit()?;
        Ok(row)
    }

    /// Readiness for a new task, without exposing another Conversation's identity.
    pub fn conference_preflight(&self, tenant: &str, cid: &str) -> Result<Value> {
        let conn = self.conn.lock().expect("store lock");
        conference_readiness(&conn, tenant, cid)
    }

    /// Retire reply routing only after all local effects have settled. Never delete history.
    pub fn close_conference(
        &self,
        tenant: &str,
        cid: &str,
        actor: &str,
        note: &str,
        request: &str,
    ) -> Result<()> {
        if note.trim().is_empty() || note.len() > 2000 {
            return Err(ApiError::bad_request(
                "verification_note must contain 1-2000 bytes",
            ));
        }
        let mut conn = self.conn.lock().expect("store lock");
        let tx = conn.transaction()?;
        let readiness = conference_readiness(&tx, tenant, cid)?;
        if readiness["state"] == "closed" {
            return Ok(());
        }
        if readiness["active_sessions"] != 0 || readiness["unsettled_sms"] != 0 {
            return Err(ApiError::conflict("end or reconcile voice and settle queued, submitting, or unknown SMS before closing"));
        }
        let now = Utc::now().to_rfc3339();
        tx.execute("UPDATE conversations SET state='closed',closed_at=?3,last_activity_at=?3 WHERE tenant_id=?1 AND id=?2", params![tenant,cid,now])?;
        append_event(
            &tx,
            tenant,
            cid,
            "conversation.closed",
            Some(request),
            &[],
            json!({"cid":cid,"closed_by":actor,"verification_note":note,"state":"closed"}),
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn create_conference(
        &self,
        tenant: &str,
        cid: &str,
        inputs: &[MemberInput],
        request: &str,
    ) -> Result<Vec<Member>> {
        validate_members(inputs)?;
        let mut conn = self.conn.lock().expect("store lock");
        let tx = conn.transaction()?;
        let now = Utc::now().to_rfc3339();
        tx.execute("INSERT INTO conversations(id,tenant_id,state,policy,opened_at,last_activity_at,metadata) VALUES(?1,?2,'open','persistent',?3,?3,?4)",
            params![cid,tenant,now,json!({"profile":"conversation-control/1"}).to_string()])?;
        let mut members = vec![];
        for input in inputs {
            let pid = format!("part_{}", Uuid::new_v4().simple());
            let subject = format!("participant:{pid}");
            let kind = if input.role == "assistant" {
                "ai"
            } else {
                "human"
            };
            tx.execute("INSERT INTO participants(id,tenant_id,conversation_id,kind,role,identity_ref,display_name,joined_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![pid,tenant,cid,kind,input.role,subject,input.name,now])?;
            tx.execute("INSERT INTO conference_members(tenant_id,conversation_id,participant_id,subject,alias,role) VALUES(?1,?2,?3,?4,?5,?6)",
                params![tenant,cid,pid,subject,input.alias,input.role])?;
            for (kind, address) in [("sms", &input.sms), ("sip", &input.sip)] {
                if let Some(address) = address {
                    tx.execute("INSERT INTO participant_endpoints(tenant_id,participant_id,kind,address) VALUES(?1,?2,?3,?4)",params![tenant,pid,kind,address])?;
                }
            }
            members.push(Member {
                participant_id: pid,
                subject,
                alias: input.alias.clone(),
                name: input.name.clone(),
                role: input.role.clone(),
                sms: input.sms.clone(),
                sip: input.sip.clone(),
            });
        }
        append_event(
            &tx,
            tenant,
            cid,
            "conversation.opened",
            Some(request),
            &[],
            json!({"cid":cid,"participants":members.iter().map(|m| json!({"id":m.participant_id,"name":m.name,"role":m.role})).collect::<Vec<_>>()}),
        )?;
        tx.commit()?;
        Ok(members)
    }

    pub fn conference_members(&self, tenant: &str, cid: &str) -> Result<Vec<Member>> {
        let conn = self.conn.lock().expect("store lock");
        let mut stmt=conn.prepare("SELECT m.participant_id,m.subject,m.alias,p.display_name,m.role,(SELECT address FROM participant_endpoints e WHERE e.tenant_id=m.tenant_id AND e.participant_id=m.participant_id AND e.kind='sms'),(SELECT address FROM participant_endpoints e WHERE e.tenant_id=m.tenant_id AND e.participant_id=m.participant_id AND e.kind='sip') FROM conference_members m JOIN participants p ON p.id=m.participant_id WHERE m.tenant_id=?1 AND m.conversation_id=?2 ORDER BY m.rowid")?;
        let rows = stmt
            .query_map(params![tenant, cid], |r| {
                Ok(Member {
                    participant_id: r.get(0)?,
                    subject: r.get(1)?,
                    alias: r.get(2)?,
                    name: r.get(3)?,
                    role: r.get(4)?,
                    sms: r.get(5)?,
                    sip: r.get(6)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn issue_conference_token(&self, tenant: &str, cid: &str, pid: &str) -> Result<String> {
        let member = self
            .conference_members(tenant, cid)?
            .into_iter()
            .find(|m| m.participant_id == pid)
            .ok_or_else(|| ApiError::not_found("participant not found"))?;
        let token = format!("pc_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        self.conn.lock().expect("store lock").execute("INSERT INTO conference_tokens(token_hash,tenant_id,subject,expires_at) VALUES(?1,?2,?3,?4)",params![crate::config::hash_secret(&token),tenant,member.subject,(Utc::now()+Duration::hours(12)).to_rfc3339()])?;
        Ok(token)
    }

    pub fn conference_principal(
        &self,
        token: &str,
    ) -> Result<Option<(String, String, chrono::DateTime<Utc>)>> {
        let raw: Option<(String, String, String)> = self
            .conn
            .lock()
            .expect("store lock")
            .query_row(
                "SELECT tenant_id,subject,expires_at FROM conference_tokens WHERE token_hash=?1",
                params![crate::config::hash_secret(token)],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((tenant, subject, expires)) = raw else {
            return Ok(None);
        };
        let expires = chrono::DateTime::parse_from_rfc3339(&expires)
            .map_err(|_| ApiError::internal("invalid stored token expiry"))?
            .with_timezone(&Utc);
        if expires <= Utc::now() {
            return Err(ApiError::unauthorized("conference token expired"));
        }
        Ok(Some((tenant, subject, expires)))
    }

    pub fn cached_command(
        &self,
        tenant: &str,
        subject: &str,
        id: &str,
        fingerprint: &str,
    ) -> Result<Option<Option<Value>>> {
        let row:Option<(String,Option<String>)>=self.conn.lock().expect("store lock").query_row("SELECT fingerprint,response FROM conference_requests WHERE tenant_id=?1 AND subject=?2 AND request_id=?3",params![tenant,subject,id],|r| Ok((r.get(0)?,r.get(1)?))).optional()?;
        match row {
            None => Ok(None),
            Some((stored, _)) if stored != fingerprint => Err(ApiError::conflict(
                "request ID reused with different content",
            )),
            Some((_, response)) => Ok(Some(
                response
                    .map(|r| {
                        serde_json::from_str(&r)
                            .map_err(|_| ApiError::internal("invalid cached response"))
                    })
                    .transpose()?,
            )),
        }
    }

    pub fn begin_command(
        &self,
        tenant: &str,
        subject: &str,
        id: &str,
        fingerprint: &str,
        envelope: Value,
    ) -> Result<()> {
        let mut conn = self.conn.lock().expect("store lock");
        let tx = conn.transaction()?;
        tx.execute("INSERT INTO conference_requests(tenant_id,subject,request_id,fingerprint,created_at) VALUES(?1,?2,?3,?4,?5)",params![tenant,subject,id,fingerprint,Utc::now().to_rfc3339()])?;
        tx.execute("INSERT INTO conference_request_evidence(tenant_id,subject,request_id,envelope) VALUES(?1,?2,?3,?4)",params![tenant,subject,id,envelope.to_string()])?;
        tx.commit()?;
        Ok(())
    }

    pub fn inspect_conference_command(
        &self,
        tenant: &str,
        cid: &str,
        request: &str,
    ) -> Result<Value> {
        let conn = self.conn.lock().expect("store lock");
        let mut stmt=conn.prepare("SELECT e.subject,e.envelope,r.response FROM conference_request_evidence e JOIN conference_requests r USING(tenant_id,subject,request_id) WHERE e.tenant_id=?1 AND e.request_id=?2 AND json_extract(e.envelope,'$.cid')=?3")?;
        let rows = stmt
            .query_map(params![tenant, request, cid], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if rows.len() > 1 {
            return Err(ApiError::conflict(
                "request ID is ambiguous across principals",
            ));
        }
        let (subject, request, response) = rows.into_iter().next().ok_or_else(|| {
            ApiError::not_found("request evidence not found in this Conversation")
        })?;
        Ok(crate::conference_network::redact_evidence(
            json!({"actor_subject":subject,"request":serde_json::from_str::<Value>(&request).map_err(|_|ApiError::internal("invalid request evidence"))?,"response":response.and_then(|s|serde_json::from_str::<Value>(&s).ok())}),
        ))
    }

    pub fn finish_command(
        &self,
        tenant: &str,
        subject: &str,
        id: &str,
        response: Value,
    ) -> Result<()> {
        self.conn.lock().expect("store lock").execute("UPDATE conference_requests SET response=?4 WHERE tenant_id=?1 AND subject=?2 AND request_id=?3",params![tenant,subject,id,response.to_string()])?;
        Ok(())
    }

    pub fn enqueue_conference_message(
        &self,
        tenant: &str,
        cid: &str,
        actor: &Member,
        recipients: &[Member],
        message_id: &str,
        body: &str,
        content_type: &str,
        medium: &str,
        reply_to: Option<&str>,
        request: &str,
        sms_from: &str,
    ) -> Result<Value> {
        if body.is_empty() || body.len() > 16000 || recipients.is_empty() || recipients.len() > 32 {
            return Err(ApiError::bad_request("message body or recipients invalid"));
        }
        if medium != "sms" && medium != "chat" {
            return Err(ApiError::bad_request("delivery must be sms or chat"));
        }
        if medium == "sms"
            && (content_type != "text/plain" || recipients.iter().any(|r| r.sms.is_none()))
        {
            return Err(ApiError::bad_request(
                "SMS requires text/plain and an SMS endpoint for every recipient",
            ));
        }
        let mut conn = self.conn.lock().expect("store lock");
        let tx = conn.transaction()?;
        let open:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM conversations WHERE tenant_id=?1 AND id=?2 AND state='open')",params![tenant,cid],|r| r.get(0))?;
        if !open {
            return Err(ApiError::conflict("conversation is not open"));
        }
        if let Some(reply) = reply_to {
            let exists:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM messages m WHERE m.tenant_id=?1 AND m.conversation_id=?2 AND m.id=?3 AND (?4 OR m.from_participant=?5 OR EXISTS(SELECT 1 FROM message_recipients r WHERE r.tenant_id=?1 AND r.message_id=m.id AND r.participant_id=?5)))",params![tenant,cid,reply,actor.observes_all(),actor.participant_id],|r|r.get(0))?;
            if !exists {
                return Err(ApiError::bad_request(
                    "reply target is not visible in this Conversation",
                ));
            }
        }
        let duplicate: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM messages WHERE id=?1)",
            params![message_id],
            |r| r.get(0),
        )?;
        if duplicate {
            return Err(ApiError::conflict(
                "message ID already exists; replay its original request",
            ));
        }
        let now = Utc::now().to_rfc3339();
        tx.execute("INSERT INTO messages(id,tenant_id,conversation_id,from_participant,medium,body,state,created_at) VALUES(?1,?2,?3,?4,?5,?6,'accepted',?7)",params![message_id,tenant,cid,actor.participant_id,medium,body,now])?;
        tx.execute("INSERT INTO message_metadata(message_id,content_type,in_reply_to,request_id) VALUES(?1,?2,?3,?4)",params![message_id,content_type,reply_to,request])?;
        let mut audience = vec![actor.participant_id.clone()];
        let mut deliveries = vec![];
        for recipient in recipients {
            audience.push(recipient.participant_id.clone());
            tx.execute("INSERT INTO message_recipients(tenant_id,message_id,participant_id) VALUES(?1,?2,?3)",params![tenant,message_id,recipient.participant_id])?;
            if medium == "sms" {
                let delivery = format!("delivery_{}", Uuid::new_v4().simple());
                tx.execute("INSERT INTO message_deliveries(id,tenant_id,conversation_id,message_id,participant_id,sender_address,recipient_address,state,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,'queued',?8,?8)",params![delivery,tenant,cid,message_id,recipient.participant_id,sms_from,recipient.sms,now])?;
                deliveries.push(json!({"id":delivery,"participant_id":recipient.participant_id,"state":"queued"}));
            }
        }
        let result = json!({"msg_id":message_id,"from":actor.participant_id,"to":recipients.iter().map(|m|m.participant_id.clone()).collect::<Vec<_>>(),"body":body,"content_type":content_type,"delivery":medium,"in_reply_to_msg":reply_to,"state":"accepted","deliveries":deliveries});
        append_event(
            &tx,
            tenant,
            cid,
            "message.accepted",
            Some(request),
            &audience,
            result.clone(),
        )?;
        tx.execute(
            "UPDATE conversations SET last_activity_at=?3 WHERE tenant_id=?1 AND id=?2",
            params![tenant, cid, now],
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn conference_events(
        &self,
        tenant: &str,
        cid: &str,
        member: &Member,
        after: i64,
        limit: usize,
    ) -> Result<Vec<JournalEvent>> {
        let conn = self.conn.lock().expect("store lock");
        // Filter before LIMIT so invisible events do not trap a subscriber's cursor.
        let mut stmt=conn.prepare("SELECT seq,conversation_id,event_type,request_id,payload,created_at FROM conference_events WHERE tenant_id=?1 AND conversation_id=?2 AND seq>?3 AND (?4 OR audience='[]' OR EXISTS(SELECT 1 FROM json_each(audience) WHERE value=?5)) ORDER BY seq LIMIT ?6")?;
        let events = stmt
            .query_map(
                params![
                    tenant,
                    cid,
                    after,
                    member.observes_all(),
                    member.participant_id,
                    limit.min(500) as i64
                ],
                |r| {
                    let raw: String = r.get(4)?;
                    Ok(JournalEvent {
                        seq: r.get(0)?,
                        cid: r.get(1)?,
                        event_type: r.get(2)?,
                        request_id: r.get(3)?,
                        payload: serde_json::from_str(&raw).unwrap_or(Value::Null),
                        created_at: r.get(5)?,
                    })
                },
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(events)
    }

    pub fn conference_history_page(
        &self,
        tenant: &str,
        cid: &str,
        member: &Member,
        after: i64,
    ) -> Result<HistoryPage> {
        if after < 0 {
            return Err(ApiError::bad_request("invalid history cursor"));
        }
        let conn = self.conn.lock().expect("store lock");
        let mut stmt=conn.prepare("SELECT m.id,m.tenant_id,m.conversation_id,m.from_participant,m.medium,m.body,m.provider_id,m.state,m.created_at,m.rowid FROM messages m WHERE m.tenant_id=?1 AND m.conversation_id=?2 AND m.rowid>?3 AND (?4 OR m.from_participant=?5 OR EXISTS(SELECT 1 FROM message_recipients r WHERE r.tenant_id=?1 AND r.message_id=m.id AND r.participant_id=?5)) ORDER BY m.rowid LIMIT 501")?;
        let mut rows = stmt
            .query_map(
                params![
                    tenant,
                    cid,
                    after,
                    member.observes_all(),
                    member.participant_id
                ],
                |r| Ok((super::sqlite::map_message(r)?, r.get::<_, i64>(9)?)),
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let has_more = rows.len() > 500;
        rows.truncate(500);
        let cursor = rows.last().map(|(_, seq)| *seq).unwrap_or(after);
        Ok(HistoryPage {
            messages: rows.into_iter().map(|(message, _)| message).collect(),
            cursor,
            has_more,
        })
    }

    pub fn conference_history(
        &self,
        tenant: &str,
        cid: &str,
        member: &Member,
    ) -> Result<Vec<MessageRow>> {
        let messages = self.list_messages(tenant, cid)?;
        if member.observes_all() {
            return Ok(messages);
        }
        let conn = self.conn.lock().expect("store lock");
        let mut visible = vec![];
        for m in messages {
            let recipient:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM message_recipients WHERE tenant_id=?1 AND message_id=?2 AND participant_id=?3)",params![tenant,m.id,member.participant_id],|r|r.get(0))?;
            if recipient || m.from_participant.as_deref() == Some(&member.participant_id) {
                visible.push(m);
            }
        }
        Ok(visible)
    }
}

fn map_delivery(r: &rusqlite::Row<'_>) -> rusqlite::Result<Delivery> {
    Ok(Delivery {
        id: r.get(0)?,
        tenant_id: r.get(1)?,
        conversation_id: r.get(2)?,
        message_id: r.get(3)?,
        participant_id: r.get(4)?,
        sender_address: r.get(5)?,
        recipient_address: r.get(6)?,
        provider_id: r.get(7)?,
        state: r.get(8)?,
        error: r.get(9)?,
        body: r.get(10)?,
    })
}

fn persist_inbound_sms(
    conn: &Connection,
    tenant: &str,
    provider_id: &str,
    body: &str,
    cid: &str,
    pid: &str,
    request: Option<&str>,
    routing: Value,
) -> Result<MessageRow> {
    let recipients: Vec<String> = {
        let mut stmt=conn.prepare("SELECT participant_id FROM conference_members WHERE tenant_id=?1 AND conversation_id=?2 AND role IN ('owner','assistant')")?;
        let rows = stmt
            .query_map(params![tenant, cid], |r| r.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows
    };
    let row = MessageRow {
        id: format!("msg_{}", Uuid::new_v4().simple()),
        tenant_id: tenant.into(),
        conversation_id: cid.into(),
        from_participant: Some(pid.into()),
        medium: "sms".into(),
        body: body.into(),
        provider_id: Some(provider_id.into()),
        state: "received".into(),
        created_at: Utc::now().to_rfc3339(),
    };
    conn.execute("INSERT INTO messages(id,tenant_id,conversation_id,from_participant,medium,body,provider_id,state,created_at) VALUES(?1,?2,?3,?4,'sms',?5,?6,'received',?7)",params![row.id,tenant,cid,pid,body,provider_id,row.created_at])?;
    conn.execute("INSERT INTO message_metadata(message_id,content_type,request_id) VALUES(?1,'text/plain',?2)",params![row.id,provider_id])?;
    for recipient in &recipients {
        conn.execute(
            "INSERT INTO message_recipients(tenant_id,message_id,participant_id) VALUES(?1,?2,?3)",
            params![tenant, row.id, recipient],
        )?;
    }
    conn.execute(
        "INSERT INTO conference_inbound_sms(tenant_id,provider_id,message_id) VALUES(?1,?2,?3)",
        params![tenant, provider_id, row.id],
    )?;
    let mut audience = recipients.clone();
    audience.push(pid.into());
    append_event(
        conn,
        tenant,
        cid,
        "message.received",
        request,
        &audience,
        json!({"msg_id":row.id,"from":pid,"to":recipients,"body":body,"content_type":"text/plain","delivery":"sms","state":"received","routing":routing}),
    )?;
    Ok(row)
}

fn conference_readiness(conn: &Connection, tenant: &str, cid: &str) -> Result<Value> {
    let state: String = conn
        .query_row(
            "SELECT state FROM conversations WHERE tenant_id=?1 AND id=?2",
            params![tenant, cid],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| ApiError::not_found("Conversation not found"))?;
    let active: i64 = conn.query_row("SELECT count(*) FROM sessions WHERE tenant_id=?1 AND conversation_id=?2 AND ended_at IS NULL", params![tenant,cid], |r| r.get(0))?;
    let unsettled: i64 = conn.query_row("SELECT count(*) FROM message_deliveries WHERE tenant_id=?1 AND conversation_id=?2 AND state IN ('queued','submitting','unknown')", params![tenant,cid], |r| r.get(0))?;
    // Conservatively flag any other open conference using these SMS endpoints,
    // even before it sends its first message. Do not leak IDs or phone numbers.
    let overlaps: i64 = conn.query_row("SELECT count(DISTINCT other.conversation_id) FROM conference_members mine JOIN participant_endpoints a ON a.tenant_id=mine.tenant_id AND a.participant_id=mine.participant_id AND a.kind='sms' JOIN participant_endpoints b ON b.tenant_id=a.tenant_id AND b.kind='sms' AND b.address=a.address JOIN conference_members other ON other.tenant_id=b.tenant_id AND other.participant_id=b.participant_id JOIN conversations c ON c.tenant_id=other.tenant_id AND c.id=other.conversation_id WHERE mine.tenant_id=?1 AND mine.conversation_id=?2 AND other.conversation_id<>?2 AND c.state='open'", params![tenant,cid], |r| r.get(0))?;
    Ok(
        json!({"state":state,"active_sessions":active,"unsettled_sms":unsettled,"overlapping_conversations":overlaps,
        "ready_for_new_task":state=="open" && active==0 && unsettled==0 && overlaps==0}),
    )
}
