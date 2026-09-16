use crate::error::{ApiError, Result};
use crate::observe;
use crate::runtime::AppState;
use crate::store::{ConnectionRow, ParticipantRow};
use rvoip_core::ids::{ParticipantId, SessionId};
use rvoip_core::participant::{ParticipantKind, ParticipantRole};
use serde::Deserialize;
use serde_json::json;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickupState {
    Idle,
    Requested,
    Accepted,
    ReturnedToAi,
}

#[derive(Deserialize, Default)]
pub struct AcceptPickup {
    pub session_id: Option<String>,
    pub operator_participant_id: Option<String>,
}

pub async fn request(state: &AppState, tenant_id: &str, cid: &str, sid: Option<&str>) -> Result<()> {
    state.store.insert_event(
        tenant_id,
        Some(cid),
        "pickup.requested",
        json!({ "sid": sid }),
    )?;
    Ok(())
}

pub async fn accept(
    state: &AppState,
    tenant_id: &str,
    cid: &str,
    body: AcceptPickup,
) -> Result<serde_json::Value> {
    let conv = state
        .store
        .get_conversation(tenant_id, cid)?
        .ok_or_else(|| ApiError::not_found("conversation not found"))?;
    let _ = conv;
    let sid = match body.session_id {
        Some(s) => s,
        None => state
            .store
            .live_sessions_with_medium(tenant_id, cid, "voice")?
            .into_iter()
            .next()
            .map(|s| s.id)
            .ok_or_else(|| ApiError::conflict("no live voice session"))?,
    };
    let operator = if let Some(pid) = body.operator_participant_id {
        state
            .store
            .get_participant(tenant_id, &pid)?
            .ok_or_else(|| ApiError::not_found("operator participant not found"))?
    } else {
        let row = ParticipantRow {
            id: ParticipantId::new().to_string(),
            tenant_id: tenant_id.into(),
            conversation_id: cid.into(),
            kind: "human".into(),
            role: "agent".into(),
            identity_ref: None,
            display_name: Some("operator".into()),
            joined_at: chrono::Utc::now().to_rfc3339(),
            left_at: None,
        };
        state.store.insert_participant(&row)?;
        row
    };

    state
        .orchestrator
        .take_over(
            SessionId::from_string(sid.clone()),
            ParticipantId::from_string(operator.id.clone()),
            ParticipantKind::Human,
        )
        .await
        .map_err(|e| ApiError::internal(format!("take_over: {e}")))?;

    state
        .store
        .set_participant_role(tenant_id, &operator.id, "agent")?;
    for p in state.store.list_participants(tenant_id, cid)? {
        if p.kind == "ai" {
            state
                .store
                .set_participant_role(tenant_id, &p.id, "observer")?;
            let _ = state
                .orchestrator
                .set_participant_role(
                    ParticipantId::from_string(p.id.clone()),
                    ParticipantRole::Observer,
                )
                .await;
        }
    }

    let operator_conn = ConnectionRow {
        id: format!("conn_operator_{sid}"),
        tenant_id: tenant_id.into(),
        session_id: sid.clone(),
        participant_id: operator.id.clone(),
        transport: "websocket".into(),
        state: "connected".into(),
    };
    state.store.insert_connection(&operator_conn)?;
    state.store.insert_event(
        tenant_id,
        Some(cid),
        "pickup.accepted",
        json!({
            "operator_participant_id": operator.id,
            "session_id": sid
        }),
    )?;
    observe::pickup();
    Ok(json!({
        "state": "accepted",
        "session_id": sid,
        "operator_participant_id": operator.id,
        "operator_connection_id": operator_conn.id
    }))
}

pub async fn return_to_ai(state: &AppState, tenant_id: &str, cid: &str) -> Result<serde_json::Value> {
    let mut ai_id = None;
    let mut human_agents = Vec::new();
    for p in state.store.list_participants(tenant_id, cid)? {
        if p.kind == "ai" {
            ai_id = Some(p.id.clone());
            state.store.set_participant_role(tenant_id, &p.id, "agent")?;
            let _ = state
                .orchestrator
                .set_participant_role(
                    ParticipantId::from_string(p.id.clone()),
                    ParticipantRole::Agent,
                )
                .await;
        } else if p.kind == "human" && p.role == "agent" {
            human_agents.push(p.id.clone());
        }
    }
    for hid in &human_agents {
        state.store.set_participant_role(tenant_id, hid, "observer")?;
        let _ = state
            .orchestrator
            .set_participant_role(
                ParticipantId::from_string(hid.clone()),
                ParticipantRole::Observer,
            )
            .await;
    }
    state.store.insert_event(
        tenant_id,
        Some(cid),
        "participant.role_changed",
        json!({ "to": "agent", "participant_id": ai_id }),
    )?;
    Ok(json!({
        "state": "returned_to_ai",
        "ai_participant_id": ai_id
    }))
}
