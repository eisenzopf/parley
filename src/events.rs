use crate::auth::Actor;
use crate::error::Result;
use crate::runtime::AppState;
use crate::store::Store;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::broadcast;

#[derive(Clone, Debug, Serialize)]
pub struct LiveEvent {
    pub tenant_id: String,
    pub conversation_id: Option<String>,
    pub verb: String,
}

pub fn channel() -> broadcast::Sender<LiveEvent> {
    let (tx, _) = broadcast::channel(256);
    tx
}

pub fn emit(
    state: &AppState,
    tenant_id: &str,
    conversation_id: Option<&str>,
    verb: &str,
    payload: Value,
) -> Result<String> {
    let id = state
        .store
        .insert_event(tenant_id, conversation_id, verb, payload)?;
    publish(state, tenant_id, conversation_id, verb);
    Ok(id)
}

pub fn publish(state: &AppState, tenant_id: &str, conversation_id: Option<&str>, verb: &str) {
    tracing::info!(tenant_id, conversation_id, verb, "parley event");
    let _ = state.live_events.send(LiveEvent {
        tenant_id: tenant_id.into(),
        conversation_id: conversation_id.map(str::to_string),
        verb: verb.into(),
    });
}

pub fn visible_to(store: &Store, tenant_id: &str, actor: &Actor, ev: &LiveEvent) -> bool {
    if ev.tenant_id != tenant_id {
        return false;
    }
    match actor {
        Actor::ApiSecret | Actor::Operator { .. } => true,
        Actor::Widget { visitor_id, .. } => {
            let (Some(cid), Some(vid)) = (ev.conversation_id.as_deref(), visitor_id.as_deref())
            else {
                return false;
            };
            widget_owns(store, tenant_id, cid, vid)
        }
    }
}

fn widget_owns(store: &Store, tenant_id: &str, cid: &str, visitor_id: &str) -> bool {
    store
        .identities_for_conversation(tenant_id, cid)
        .ok()
        .map(|ids| {
            ids.iter().any(|i| {
                (i.key_type == "visitor_id" || i.key_type == "cookie") && i.key_value == visitor_id
            })
        })
        .unwrap_or(false)
}
