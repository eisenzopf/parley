//! Durable owner callback admission and the current speaking route.
use super::{conference::append_event, conference::Member, Store};
use crate::{ApiError, Result};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpeakingRoute {
    pub connection_id: String,
    pub participant_id: String,
    pub bridge_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PhoneMove {
    pub connection_id: String,
    pub conversation_id: String,
    pub session_id: String,
    pub participant_id: String,
    pub request_id: String,
    pub source_connection_id: String,
    pub source_bridge_id: String,
    pub state: String,
}

impl Store {
    pub fn speaking_route(&self, tenant: &str, sid: &str) -> Result<Option<SpeakingRoute>> {
        self.conn.lock().expect("store lock").query_row(
            "SELECT connection_id,participant_id,bridge_id FROM conference_speaking_routes WHERE tenant_id=?1 AND session_id=?2",
            params![tenant,sid], |r| Ok(SpeakingRoute { connection_id:r.get(0)?,participant_id:r.get(1)?,bridge_id:r.get(2)? })
        ).optional().map_err(Into::into)
    }

    pub fn phone_move(&self, tenant: &str, connid: &str) -> Result<Option<PhoneMove>> {
        self.conn.lock().expect("store lock").query_row(
            "SELECT connection_id,conversation_id,session_id,participant_id,request_id,source_connection_id,source_bridge_id,state FROM conference_phone_moves WHERE tenant_id=?1 AND connection_id=?2",
            params![tenant,connid], |r| Ok(PhoneMove { connection_id:r.get(0)?,conversation_id:r.get(1)?,session_id:r.get(2)?,participant_id:r.get(3)?,request_id:r.get(4)?,source_connection_id:r.get(5)?,source_bridge_id:r.get(6)?,state:r.get(7)? })
        ).optional().map_err(Into::into)
    }

    pub fn prepare_phone_move(
        &self,
        tenant: &str,
        cid: &str,
        sid: &str,
        owner: &Member,
        route: &SpeakingRoute,
        connid: &str,
        request: &str,
    ) -> Result<()> {
        let mut conn = self.conn.lock().expect("store lock");
        let tx = conn.transaction()?;
        let admissible:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM sessions s JOIN conversations c ON c.id=s.conversation_id JOIN conference_speaking_routes r ON r.session_id=s.id JOIN conference_browser_connections b ON b.connection_id=r.connection_id WHERE s.tenant_id=?1 AND s.conversation_id=?2 AND s.id=?3 AND s.state='active' AND c.state='open' AND r.connection_id=?4 AND r.bridge_id=?5 AND r.participant_id=?6 AND b.state='speaking')",params![tenant,cid,sid,route.connection_id,route.bridge_id,owner.participant_id],|r|r.get(0))?;
        let pending:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM conference_phone_moves WHERE tenant_id=?1 AND session_id=?2 AND state IN ('prepared','dialing','answered','confirmed','committing','speaking','unknown','interrupted'))",params![tenant,sid],|r|r.get(0))?;
        if !admissible || pending {
            return Err(ApiError::conflict(
                "owner must be speaking through the browser with no phone move in progress",
            ));
        }
        tx.execute("INSERT INTO connections(id,tenant_id,session_id,participant_id,transport,state) VALUES(?1,?2,?3,?4,'sip','prepared')",params![connid,tenant,sid,owner.participant_id])?;
        tx.execute("INSERT INTO conference_phone_moves(connection_id,tenant_id,conversation_id,session_id,participant_id,request_id,source_connection_id,source_bridge_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![connid,tenant,cid,sid,owner.participant_id,request,route.connection_id,route.bridge_id])?;
        append_event(
            &tx,
            tenant,
            cid,
            "phone.prepared",
            Some(request),
            &[owner.participant_id.clone()],
            json!({"sid":sid,"connid":connid,"participant_id":owner.participant_id,"source_connid":route.connection_id,"state":"prepared","transport":"sip","confirmation":"answer and press 1"}),
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Compare-and-set: late signaling, duplicate digits and late cancellation
    /// cannot regress a committed handoff or resurrect a terminal attempt.
    pub fn transition_phone_move(
        &self,
        tenant: &str,
        connid: &str,
        allowed: &[&str],
        next: &str,
        details: Value,
    ) -> Result<bool> {
        let mut conn = self.conn.lock().expect("store lock");
        let tx = conn.transaction()?;
        let found:Option<(String,String,String,String,String)>=tx.query_row("SELECT conversation_id,session_id,participant_id,request_id,state FROM conference_phone_moves WHERE tenant_id=?1 AND connection_id=?2",params![tenant,connid],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
        let Some((cid, sid, pid, request, prior)) = found else {
            return Ok(false);
        };
        if !allowed.contains(&prior.as_str()) || prior == next {
            return Ok(false);
        }
        tx.execute(
            "UPDATE conference_phone_moves SET state=?3 WHERE tenant_id=?1 AND connection_id=?2",
            params![tenant, connid, next],
        )?;
        let connection_state = match next {
            "answered" | "confirmed" | "committing" | "speaking" => "connected",
            "cancelled" => "ended",
            "unknown" | "interrupted" => "unknown",
            other => other,
        };
        tx.execute(
            "UPDATE connections SET state=?3 WHERE tenant_id=?1 AND id=?2",
            params![tenant, connid, connection_state],
        )?;
        append_event(
            &tx,
            tenant,
            &cid,
            &format!("phone.{next}"),
            Some(&request),
            &[pid.clone()],
            json!({"sid":sid,"connid":connid,"participant_id":pid,"state":next,"transport":"sip","details":details}),
        )?;
        tx.commit()?;
        Ok(true)
    }

    pub fn commit_phone_move(
        &self,
        tenant: &str,
        phone: &PhoneMove,
        retained: &str,
        bridge: &str,
    ) -> Result<()> {
        let mut conn = self.conn.lock().expect("store lock");
        let tx = conn.transaction()?;
        let changed=tx.execute("UPDATE conference_speaking_routes SET connection_id=?3,bridge_id=?4 WHERE tenant_id=?1 AND session_id=?2 AND connection_id=?5 AND bridge_id=?6 AND participant_id=?7 AND EXISTS(SELECT 1 FROM conference_phone_moves p WHERE p.connection_id=?3 AND p.state='committing')",params![tenant,phone.session_id,phone.connection_id,bridge,phone.source_connection_id,phone.source_bridge_id,phone.participant_id])?;
        if changed != 1 {
            return Err(ApiError::conflict(
                "committed phone route needs reconciliation",
            ));
        }
        tx.execute("UPDATE conference_phone_moves SET state='speaking' WHERE tenant_id=?1 AND connection_id=?2",params![tenant,phone.connection_id])?;
        tx.execute("UPDATE conference_browser_connections SET state='retired' WHERE tenant_id=?1 AND connection_id=?2",params![tenant,phone.source_connection_id])?;
        append_event(
            &tx,
            tenant,
            &phone.conversation_id,
            "phone.speaking",
            Some(&phone.request_id),
            &[phone.participant_id.clone()],
            json!({"sid":phone.session_id,"connid":phone.connection_id,"participant_id":phone.participant_id,"state":"speaking","transport":"sip","details":{"retained_connid":retained,"retired_connid":phone.source_connection_id,"bridge_id":bridge,"join_confirmed":true}}),
        )?;
        tx.commit()?;
        Ok(())
    }
}
