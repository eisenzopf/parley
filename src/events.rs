use crate::store::Store;
use serde_json::Value;

pub fn emit(store: &Store, tenant_id: &str, conversation_id: Option<&str>, verb: &str, payload: Value) {
    let _ = store.insert_event(tenant_id, conversation_id, verb, payload);
    tracing::info!(tenant_id, conversation_id, verb, "parley event");
}
