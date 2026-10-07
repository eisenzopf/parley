use crate::config::Config;
use crate::error::{ApiError, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Mutex;
use uuid::Uuid;

pub struct Store {
    pub(super) conn: Mutex<Connection>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConversationRow {
    pub id: String,
    pub tenant_id: String,
    pub state: String,
    pub policy: String,
    pub opened_at: String,
    pub closed_at: Option<String>,
    pub last_activity_at: String,
    pub vapi_chat_session_id: Option<String>,
    pub metadata: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IdentityRow {
    pub tenant_id: String,
    pub key_type: String,
    pub key_value: String,
    pub conversation_id: String,
    pub do_not_reopen: bool,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParticipantRow {
    pub id: String,
    pub tenant_id: String,
    pub conversation_id: String,
    pub kind: String,
    pub role: String,
    pub identity_ref: Option<String>,
    pub display_name: Option<String>,
    pub joined_at: String,
    pub left_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionRow {
    pub id: String,
    pub tenant_id: String,
    pub conversation_id: String,
    pub medium: String,
    pub state: String,
    pub started_at: String,
    pub ended_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectionRow {
    pub id: String,
    pub tenant_id: String,
    pub session_id: String,
    pub participant_id: String,
    pub transport: String,
    pub state: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MessageRow {
    pub id: String,
    pub tenant_id: String,
    pub conversation_id: String,
    pub from_participant: Option<String>,
    pub medium: String,
    pub body: String,
    pub provider_id: Option<String>,
    pub state: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventRow {
    pub id: String,
    pub tenant_id: String,
    pub conversation_id: Option<String>,
    pub event_type: String,
    pub payload: Value,
    pub created_at: String,
}

impl Store {
    pub fn open(config: &Config) -> Result<Self> {
        if let Some(parent) = std::path::Path::new(&config.sqlite_path).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| ApiError::internal(format!("create sqlite dir: {e}")))?;
            }
        }
        std::fs::create_dir_all(&config.blob_dir)
            .map_err(|e| ApiError::internal(format!("create blob dir: {e}")))?;
        let conn = Connection::open(&config.sqlite_path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(include_str!("../../migrations/001_init.sql"))?;
        conn.execute_batch(include_str!("../../migrations/002_conference.sql"))?;
        conn.execute_batch(include_str!("../../migrations/003_conference_voice.sql"))?;
        conn.execute_batch(include_str!("../../migrations/004_conference_browser.sql"))?;
        conn.execute_batch(include_str!("../../migrations/005_conference_evidence.sql"))?;
        conn.execute_batch(include_str!("../../migrations/006_conference_receipts.sql"))?;
        conn.execute_batch(include_str!("../../migrations/007_conference_inbox.sql"))?;
        conn.execute_batch(include_str!("../../migrations/008_conference_phone.sql"))?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.bootstrap_tenant(config)?;
        Ok(store)
    }

    fn bootstrap_tenant(&self, config: &Config) -> Result<()> {
        let conn = self.conn.lock().expect("store lock");
        let exists: Option<String> = conn
            .query_row(
                "SELECT id FROM tenants WHERE id = ?1",
                params![config.tenant_id],
                |r| r.get(0),
            )
            .optional()?;
        if exists.is_none() {
            conn.execute(
                "INSERT INTO tenants (id, api_secret_hash, created_at, config) VALUES (?1, ?2, ?3, '{}')",
                params![
                    config.tenant_id,
                    config.hash_api_secret(),
                    Utc::now().to_rfc3339()
                ],
            )?;
        } else {
            conn.execute(
                "UPDATE tenants SET api_secret_hash = ?2 WHERE id = ?1",
                params![config.tenant_id, config.hash_api_secret()],
            )?;
        }
        Ok(())
    }

    pub fn api_secret_hash(&self, tenant_id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().expect("store lock");
        conn.query_row(
            "SELECT api_secret_hash FROM tenants WHERE id = ?1",
            params![tenant_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn tenant_config(&self, tenant_id: &str) -> Result<Value> {
        let conn = self.conn.lock().expect("store lock");
        let raw: String = conn.query_row(
            "SELECT config FROM tenants WHERE id = ?1",
            params![tenant_id],
            |r| r.get(0),
        )?;
        Ok(serde_json::from_str(&raw).unwrap_or_else(|_| json!({})))
    }

    pub fn set_tenant_config(&self, tenant_id: &str, config: &Value) -> Result<()> {
        let conn = self.conn.lock().expect("store lock");
        conn.execute(
            "UPDATE tenants SET config = ?2 WHERE id = ?1",
            params![tenant_id, config.to_string()],
        )?;
        Ok(())
    }

    pub fn lookup_identity(
        &self,
        tenant_id: &str,
        key_type: &str,
        key_value: &str,
    ) -> Result<Option<IdentityRow>> {
        let conn = self.conn.lock().expect("store lock");
        conn.query_row(
            "SELECT tenant_id, key_type, key_value, conversation_id, do_not_reopen, updated_at
             FROM identities WHERE tenant_id = ?1 AND key_type = ?2 AND key_value = ?3",
            params![tenant_id, key_type, key_value],
            |r| {
                Ok(IdentityRow {
                    tenant_id: r.get(0)?,
                    key_type: r.get(1)?,
                    key_value: r.get(2)?,
                    conversation_id: r.get(3)?,
                    do_not_reopen: r.get::<_, i64>(4)? != 0,
                    updated_at: r.get(5)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn upsert_identity(
        &self,
        tenant_id: &str,
        key_type: &str,
        key_value: &str,
        conversation_id: &str,
        do_not_reopen: bool,
    ) -> Result<()> {
        let conn = self.conn.lock().expect("store lock");
        conn.execute(
            "INSERT INTO identities (tenant_id, key_type, key_value, conversation_id, do_not_reopen, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(tenant_id, key_type, key_value) DO UPDATE SET
               conversation_id = excluded.conversation_id,
               do_not_reopen = excluded.do_not_reopen,
               updated_at = excluded.updated_at",
            params![
                tenant_id,
                key_type,
                key_value,
                conversation_id,
                if do_not_reopen { 1 } else { 0 },
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn get_conversation(&self, tenant_id: &str, id: &str) -> Result<Option<ConversationRow>> {
        let conn = self.conn.lock().expect("store lock");
        conn.query_row(
            "SELECT id, tenant_id, state, policy, opened_at, closed_at, last_activity_at, vapi_chat_session_id, metadata
             FROM conversations WHERE tenant_id = ?1 AND id = ?2",
            params![tenant_id, id],
            map_conversation,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn list_conversations(&self, tenant_id: &str) -> Result<Vec<ConversationRow>> {
        let conn = self.conn.lock().expect("store lock");
        let mut stmt = conn.prepare(
            "SELECT id, tenant_id, state, policy, opened_at, closed_at, last_activity_at, vapi_chat_session_id, metadata
             FROM conversations WHERE tenant_id = ?1 ORDER BY last_activity_at DESC",
        )?;
        let rows = stmt
            .query_map(params![tenant_id], map_conversation)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn insert_conversation(&self, row: &ConversationRow) -> Result<()> {
        let conn = self.conn.lock().expect("store lock");
        conn.execute(
            "INSERT INTO conversations (id, tenant_id, state, policy, opened_at, closed_at, last_activity_at, vapi_chat_session_id, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                row.id,
                row.tenant_id,
                row.state,
                row.policy,
                row.opened_at,
                row.closed_at,
                row.last_activity_at,
                row.vapi_chat_session_id,
                row.metadata.to_string()
            ],
        )?;
        Ok(())
    }

    pub fn touch_conversation(&self, tenant_id: &str, id: &str) -> Result<()> {
        let conn = self.conn.lock().expect("store lock");
        conn.execute(
            "UPDATE conversations SET last_activity_at = ?3 WHERE tenant_id = ?1 AND id = ?2",
            params![tenant_id, id, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn reopen_conversation(&self, tenant_id: &str, id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().expect("store lock");
        conn.execute(
            "UPDATE conversations SET state = 'open', closed_at = NULL, last_activity_at = ?3
             WHERE tenant_id = ?1 AND id = ?2",
            params![tenant_id, id, now],
        )?;
        Ok(())
    }

    pub fn close_conversation(&self, tenant_id: &str, id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().expect("store lock");
        conn.execute(
            "UPDATE conversations SET state = 'closed', closed_at = ?3, last_activity_at = ?3
             WHERE tenant_id = ?1 AND id = ?2",
            params![tenant_id, id, now],
        )?;
        Ok(())
    }

    pub fn set_vapi_chat_session(&self, tenant_id: &str, id: &str, session_id: &str) -> Result<()> {
        let conn = self.conn.lock().expect("store lock");
        conn.execute(
            "UPDATE conversations SET vapi_chat_session_id = ?3 WHERE tenant_id = ?1 AND id = ?2",
            params![tenant_id, id, session_id],
        )?;
        Ok(())
    }

    pub fn insert_participant(&self, row: &ParticipantRow) -> Result<()> {
        let conn = self.conn.lock().expect("store lock");
        conn.execute(
            "INSERT INTO participants (id, tenant_id, conversation_id, kind, role, identity_ref, display_name, joined_at, left_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                row.id,
                row.tenant_id,
                row.conversation_id,
                row.kind,
                row.role,
                row.identity_ref,
                row.display_name,
                row.joined_at,
                row.left_at
            ],
        )?;
        Ok(())
    }

    pub fn list_participants(&self, tenant_id: &str, cid: &str) -> Result<Vec<ParticipantRow>> {
        let conn = self.conn.lock().expect("store lock");
        let mut stmt = conn.prepare(
            "SELECT id, tenant_id, conversation_id, kind, role, identity_ref, display_name, joined_at, left_at
             FROM participants WHERE tenant_id = ?1 AND conversation_id = ?2 ORDER BY joined_at",
        )?;
        let rows = stmt
            .query_map(params![tenant_id, cid], |r| {
                Ok(ParticipantRow {
                    id: r.get(0)?,
                    tenant_id: r.get(1)?,
                    conversation_id: r.get(2)?,
                    kind: r.get(3)?,
                    role: r.get(4)?,
                    identity_ref: r.get(5)?,
                    display_name: r.get(6)?,
                    joined_at: r.get(7)?,
                    left_at: r.get(8)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn get_participant(&self, tenant_id: &str, pid: &str) -> Result<Option<ParticipantRow>> {
        let conn = self.conn.lock().expect("store lock");
        conn.query_row(
            "SELECT id, tenant_id, conversation_id, kind, role, identity_ref, display_name, joined_at, left_at
             FROM participants WHERE tenant_id = ?1 AND id = ?2",
            params![tenant_id, pid],
            |r| {
                Ok(ParticipantRow {
                    id: r.get(0)?,
                    tenant_id: r.get(1)?,
                    conversation_id: r.get(2)?,
                    kind: r.get(3)?,
                    role: r.get(4)?,
                    identity_ref: r.get(5)?,
                    display_name: r.get(6)?,
                    joined_at: r.get(7)?,
                    left_at: r.get(8)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn set_participant_role(&self, tenant_id: &str, pid: &str, role: &str) -> Result<()> {
        let conn = self.conn.lock().expect("store lock");
        conn.execute(
            "UPDATE participants SET role = ?3 WHERE tenant_id = ?1 AND id = ?2",
            params![tenant_id, pid, role],
        )?;
        Ok(())
    }

    pub fn insert_session(&self, row: &SessionRow) -> Result<()> {
        let conn = self.conn.lock().expect("store lock");
        conn.execute(
            "INSERT INTO sessions (id, tenant_id, conversation_id, medium, state, started_at, ended_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                row.id,
                row.tenant_id,
                row.conversation_id,
                row.medium,
                row.state,
                row.started_at,
                row.ended_at
            ],
        )?;
        Ok(())
    }

    pub fn list_sessions(&self, tenant_id: &str, cid: &str) -> Result<Vec<SessionRow>> {
        let conn = self.conn.lock().expect("store lock");
        let mut stmt = conn.prepare(
            "SELECT id, tenant_id, conversation_id, medium, state, started_at, ended_at
             FROM sessions WHERE tenant_id = ?1 AND conversation_id = ?2 ORDER BY started_at",
        )?;
        let rows = stmt
            .query_map(params![tenant_id, cid], map_session)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn get_session(&self, tenant_id: &str, sid: &str) -> Result<Option<SessionRow>> {
        let conn = self.conn.lock().expect("store lock");
        conn.query_row(
            "SELECT id, tenant_id, conversation_id, medium, state, started_at, ended_at
             FROM sessions WHERE tenant_id = ?1 AND id = ?2",
            params![tenant_id, sid],
            map_session,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn end_session(&self, tenant_id: &str, sid: &str) -> Result<()> {
        let conn = self.conn.lock().expect("store lock");
        conn.execute(
            "UPDATE sessions SET state = 'ended', ended_at = ?3 WHERE tenant_id = ?1 AND id = ?2",
            params![tenant_id, sid, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn live_sessions_with_medium(
        &self,
        tenant_id: &str,
        cid: &str,
        medium: &str,
    ) -> Result<Vec<SessionRow>> {
        Ok(self
            .list_sessions(tenant_id, cid)?
            .into_iter()
            .filter(|s| s.medium == medium && s.state != "ended" && s.state != "failed")
            .collect())
    }

    pub fn insert_connection(&self, row: &ConnectionRow) -> Result<()> {
        let conn = self.conn.lock().expect("store lock");
        conn.execute(
            "INSERT INTO connections (id, tenant_id, session_id, participant_id, transport, state)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET state = excluded.state",
            params![
                row.id,
                row.tenant_id,
                row.session_id,
                row.participant_id,
                row.transport,
                row.state
            ],
        )?;
        Ok(())
    }

    pub fn list_connections(
        &self,
        tenant_id: &str,
        session_id: &str,
    ) -> Result<Vec<ConnectionRow>> {
        let conn = self.conn.lock().expect("store lock");
        let mut stmt = conn.prepare(
            "SELECT id, tenant_id, session_id, participant_id, transport, state
             FROM connections WHERE tenant_id = ?1 AND session_id = ?2",
        )?;
        let rows = stmt
            .query_map(params![tenant_id, session_id], map_connection)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn delete_connection(&self, tenant_id: &str, id: &str) -> Result<()> {
        let conn = self.conn.lock().expect("store lock");
        conn.execute(
            "DELETE FROM connections WHERE tenant_id = ?1 AND id = ?2",
            params![tenant_id, id],
        )?;
        Ok(())
    }

    pub fn get_connection(&self, tenant_id: &str, id: &str) -> Result<Option<ConnectionRow>> {
        let conn = self.conn.lock().expect("store lock");
        conn.query_row(
            "SELECT id, tenant_id, session_id, participant_id, transport, state
             FROM connections WHERE tenant_id = ?1 AND id = ?2",
            params![tenant_id, id],
            map_connection,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn count_live_voice_sessions(&self, tenant_id: &str, cid: &str) -> Result<i64> {
        let conn = self.conn.lock().expect("store lock");
        conn.query_row(
            "SELECT COUNT(*) FROM sessions
             WHERE tenant_id = ?1 AND conversation_id = ?2 AND medium = 'voice'
               AND state NOT IN ('ended', 'failed')",
            params![tenant_id, cid],
            |r| r.get(0),
        )
        .map_err(Into::into)
    }

    pub fn insert_message(&self, row: &MessageRow) -> Result<()> {
        let conn = self.conn.lock().expect("store lock");
        conn.execute(
            "INSERT INTO messages (id, tenant_id, conversation_id, from_participant, medium, body, provider_id, state, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                row.id,
                row.tenant_id,
                row.conversation_id,
                row.from_participant,
                row.medium,
                row.body,
                row.provider_id,
                row.state,
                row.created_at
            ],
        )?;
        Ok(())
    }

    pub fn set_message_delivery(
        &self,
        tenant_id: &str,
        id: &str,
        provider_id: Option<&str>,
        state: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().expect("store lock");
        conn.execute(
            "UPDATE messages SET provider_id = COALESCE(?3, provider_id), state = ?4
             WHERE tenant_id = ?1 AND id = ?2",
            params![tenant_id, id, provider_id, state],
        )?;
        Ok(())
    }

    pub fn find_message_by_provider(
        &self,
        tenant_id: &str,
        provider_id: &str,
    ) -> Result<Option<MessageRow>> {
        let conn = self.conn.lock().expect("store lock");
        conn.query_row(
            "SELECT id, tenant_id, conversation_id, from_participant, medium, body, provider_id, state, created_at
             FROM messages WHERE tenant_id = ?1 AND provider_id = ?2",
            params![tenant_id, provider_id],
            map_message,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn get_message(&self, tenant_id: &str, id: &str) -> Result<Option<MessageRow>> {
        let conn = self.conn.lock().expect("store lock");
        conn.query_row(
            "SELECT id, tenant_id, conversation_id, from_participant, medium, body, provider_id, state, created_at
             FROM messages WHERE tenant_id = ?1 AND id = ?2",
            params![tenant_id, id],
            map_message,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn list_messages(&self, tenant_id: &str, cid: &str) -> Result<Vec<MessageRow>> {
        let conn = self.conn.lock().expect("store lock");
        let mut stmt = conn.prepare(
            "SELECT id, tenant_id, conversation_id, from_participant, medium, body, provider_id, state, created_at
             FROM messages WHERE tenant_id = ?1 AND conversation_id = ?2 ORDER BY created_at",
        )?;
        let rows = stmt
            .query_map(params![tenant_id, cid], map_message)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn insert_event(
        &self,
        tenant_id: &str,
        conversation_id: Option<&str>,
        event_type: &str,
        payload: Value,
    ) -> Result<String> {
        let id = format!("evt_{}", Uuid::new_v4().simple());
        let conn = self.conn.lock().expect("store lock");
        conn.execute(
            "INSERT INTO events (id, tenant_id, conversation_id, type, payload, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id,
                tenant_id,
                conversation_id,
                event_type,
                payload.to_string(),
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(id)
    }

    pub fn list_events(&self, tenant_id: &str, cid: &str) -> Result<Vec<EventRow>> {
        let conn = self.conn.lock().expect("store lock");
        let mut stmt = conn.prepare(
            "SELECT id, tenant_id, conversation_id, type, payload, created_at
             FROM events WHERE tenant_id = ?1 AND conversation_id = ?2 ORDER BY created_at",
        )?;
        let rows = stmt
            .query_map(params![tenant_id, cid], |r| {
                let payload: String = r.get(4)?;
                Ok(EventRow {
                    id: r.get(0)?,
                    tenant_id: r.get(1)?,
                    conversation_id: r.get(2)?,
                    event_type: r.get(3)?,
                    payload: serde_json::from_str(&payload).unwrap_or(Value::Null),
                    created_at: r.get(5)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn insert_vcon(
        &self,
        tenant_id: &str,
        conversation_id: Option<&str>,
        session_id: Option<&str>,
        path: &str,
    ) -> Result<String> {
        let id = format!("vcon_{}", Uuid::new_v4().simple());
        let conn = self.conn.lock().expect("store lock");
        conn.execute(
            "INSERT INTO vcons (id, tenant_id, conversation_id, session_id, path, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id,
                tenant_id,
                conversation_id,
                session_id,
                path,
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(id)
    }

    pub fn get_vcon_path(&self, tenant_id: &str, cid: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().expect("store lock");
        conn.query_row(
            "SELECT path FROM vcons WHERE tenant_id = ?1 AND conversation_id = ?2
             ORDER BY created_at DESC LIMIT 1",
            params![tenant_id, cid],
            |r| r.get(0),
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn operator_count(&self, tenant_id: &str) -> Result<i64> {
        let conn = self.conn.lock().expect("store lock");
        conn.query_row(
            "SELECT COUNT(*) FROM operators WHERE tenant_id = ?1",
            params![tenant_id],
            |r| r.get(0),
        )
        .map_err(Into::into)
    }

    pub fn insert_operator(&self, tenant_id: &str, email: &str) -> Result<String> {
        let id = format!("opr_{}", Uuid::new_v4().simple());
        let conn = self.conn.lock().expect("store lock");
        conn.execute(
            "INSERT INTO operators (id, tenant_id, email, password_hash, created_at)
             VALUES (?1, ?2, ?3, NULL, ?4)",
            params![id, tenant_id, email, Utc::now().to_rfc3339()],
        )?;
        Ok(id)
    }

    pub fn find_operator_by_email(&self, tenant_id: &str, email: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().expect("store lock");
        conn.query_row(
            "SELECT id FROM operators WHERE tenant_id = ?1 AND email = ?2",
            params![tenant_id, email],
            |r| r.get(0),
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn insert_operator_session(
        &self,
        tenant_id: &str,
        operator_id: &str,
        token_hash: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<()> {
        let conn = self.conn.lock().expect("store lock");
        conn.execute(
            "INSERT INTO operator_sessions (token_hash, tenant_id, operator_id, expires_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![token_hash, tenant_id, operator_id, expires_at.to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn identities_for_conversation(
        &self,
        tenant_id: &str,
        cid: &str,
    ) -> Result<Vec<IdentityRow>> {
        let conn = self.conn.lock().expect("store lock");
        let mut stmt = conn.prepare(
            "SELECT tenant_id, key_type, key_value, conversation_id, do_not_reopen, updated_at
             FROM identities WHERE tenant_id = ?1 AND conversation_id = ?2",
        )?;
        let rows = stmt
            .query_map(params![tenant_id, cid], |r| {
                Ok(IdentityRow {
                    tenant_id: r.get(0)?,
                    key_type: r.get(1)?,
                    key_value: r.get(2)?,
                    conversation_id: r.get(3)?,
                    do_not_reopen: r.get::<_, i64>(4)? != 0,
                    updated_at: r.get(5)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn lookup_operator_session(
        &self,
        token_hash: &str,
    ) -> Result<Option<(String, String, String)>> {
        let conn = self.conn.lock().expect("store lock");
        conn.query_row(
            "SELECT tenant_id, operator_id, expires_at FROM operator_sessions WHERE token_hash = ?1",
            params![token_hash],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn idempotency_get(&self, tenant_id: &str, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().expect("store lock");
        conn.query_row(
            "SELECT resource_id FROM idempotency WHERE tenant_id = ?1 AND key = ?2",
            params![tenant_id, key],
            |r| r.get(0),
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn idempotency_put(&self, tenant_id: &str, key: &str, resource_id: &str) -> Result<()> {
        let conn = self.conn.lock().expect("store lock");
        conn.execute(
            "INSERT OR IGNORE INTO idempotency (tenant_id, key, resource_id, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![tenant_id, key, resource_id, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }
}

fn map_conversation(r: &rusqlite::Row<'_>) -> rusqlite::Result<ConversationRow> {
    let metadata: String = r.get(8)?;
    Ok(ConversationRow {
        id: r.get(0)?,
        tenant_id: r.get(1)?,
        state: r.get(2)?,
        policy: r.get(3)?,
        opened_at: r.get(4)?,
        closed_at: r.get(5)?,
        last_activity_at: r.get(6)?,
        vapi_chat_session_id: r.get(7)?,
        metadata: serde_json::from_str(&metadata).unwrap_or(serde_json::json!({})),
    })
}

fn map_session(r: &rusqlite::Row<'_>) -> rusqlite::Result<SessionRow> {
    Ok(SessionRow {
        id: r.get(0)?,
        tenant_id: r.get(1)?,
        conversation_id: r.get(2)?,
        medium: r.get(3)?,
        state: r.get(4)?,
        started_at: r.get(5)?,
        ended_at: r.get(6)?,
    })
}

fn map_connection(r: &rusqlite::Row<'_>) -> rusqlite::Result<ConnectionRow> {
    Ok(ConnectionRow {
        id: r.get(0)?,
        tenant_id: r.get(1)?,
        session_id: r.get(2)?,
        participant_id: r.get(3)?,
        transport: r.get(4)?,
        state: r.get(5)?,
    })
}

pub(super) fn map_message(r: &rusqlite::Row<'_>) -> rusqlite::Result<MessageRow> {
    Ok(MessageRow {
        id: r.get(0)?,
        tenant_id: r.get(1)?,
        conversation_id: r.get(2)?,
        from_participant: r.get(3)?,
        medium: r.get(4)?,
        body: r.get(5)?,
        provider_id: r.get(6)?,
        state: r.get(7)?,
        created_at: r.get(8)?,
    })
}
