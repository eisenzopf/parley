//! Browser consultation state uses the existing route and attributed journal.
use super::{conference_phone::SpeakingRoute, Store};
use crate::Result;
use rusqlite::{params, OptionalExtension};
use serde_json::json;

impl Store {
    pub fn mark_conference_organizer_held(
        &self,
        tenant: &str,
        voice: &super::conference::VoiceOperation,
    ) -> Result<()> {
        let changed = self.conn.lock().expect("store lock").execute(
            "UPDATE conference_voice_operations SET ai_state='held' WHERE tenant_id=?1 AND session_id=?2 AND ai_state='ending'",
            params![tenant, voice.session_id],
        )?;
        if changed == 1 {
            self.conference_voice_fact(tenant, voice, "session.organizer_held", json!({"sid":voice.session_id,"connid":voice.remote_connection_id,"participant_id":voice.target_participant_id,"state":"held"}))?;
        }
        Ok(())
    }
    pub fn consultation_for_browser(
        &self,
        tenant: &str,
        connection: &str,
    ) -> Result<Option<String>> {
        self.conn.lock().expect("store lock").query_row(
            "SELECT session_id FROM conference_browser_connections WHERE tenant_id=?1 AND connection_id=?2 AND state='consulting'",
            params![tenant,connection], |r| r.get(0)
        ).optional().map_err(Into::into)
    }

    pub fn browser_consult(&self, tenant: &str, sid: &str) -> Result<Option<SpeakingRoute>> {
        self.conn.lock().expect("store lock").query_row(
            "SELECT r.connection_id,r.participant_id,r.bridge_id FROM conference_speaking_routes r
             JOIN conference_browser_connections b ON b.connection_id=r.connection_id
             JOIN sessions s ON s.id=r.session_id
             WHERE r.tenant_id=?1 AND r.session_id=?2 AND b.state='consulting' AND s.state='active'",
            params![tenant,sid], |r| Ok(SpeakingRoute { connection_id:r.get(0)?,participant_id:r.get(1)?,bridge_id:r.get(2)? })
        ).optional().map_err(Into::into)
    }

    pub fn latest_consult_reply(
        &self,
        tenant: &str,
        cid: &str,
        sid: &str,
        owner: &str,
    ) -> Result<Option<(i64, String)>> {
        self.conn.lock().expect("store lock").query_row(
            "SELECT seq,json_extract(payload,'$.text') FROM conference_events
             WHERE tenant_id=?1 AND conversation_id=?2 AND event_type='session.transcript'
             AND json_extract(payload,'$.sid')=?3 AND json_extract(payload,'$.speaker')=?4
             AND json_extract(payload,'$.is_final')=1
             AND seq > (SELECT MAX(seq) FROM conference_events WHERE tenant_id=?1 AND conversation_id=?2
                 AND event_type='browser.consulting' AND json_extract(payload,'$.sid')=?3)
             ORDER BY seq DESC LIMIT 1", params![tenant,cid,sid,owner], |r| Ok((r.get(0)?,r.get(1)?))
        ).optional().map_err(Into::into)
    }

    pub fn has_consult_transfer_intent(
        &self,
        tenant: &str,
        cid: &str,
        sid: &str,
        assistant: &str,
    ) -> Result<bool> {
        self.conn.lock().expect("store lock").query_row(
            "SELECT EXISTS(SELECT 1 FROM conference_events WHERE tenant_id=?1 AND conversation_id=?2
             AND event_type='session.assistant_action' AND json_extract(payload,'$.sid')=?3
             AND json_extract(payload,'$.participant_id')=?4 AND json_extract(payload,'$.source')='vapi'
             AND json_extract(payload,'$.action')='connect_organizer'
             AND seq > (SELECT MAX(seq) FROM conference_events WHERE tenant_id=?1 AND conversation_id=?2
                 AND event_type='browser.consulting' AND json_extract(payload,'$.sid')=?3))",
            params![tenant,cid,sid,assistant], |r| r.get(0)
        ).map_err(Into::into)
    }

    pub fn replace_consult_assistant(
        &self,
        tenant: &str,
        sid: &str,
        ai: &str,
        bridge: &str,
    ) -> Result<()> {
        self.conn.lock().expect("store lock").execute("UPDATE conference_voice_operations SET ai_connection_id=?3,bridge_id=?4 WHERE tenant_id=?1 AND session_id=?2",params![tenant,sid,ai,bridge])?;
        Ok(())
    }
}
