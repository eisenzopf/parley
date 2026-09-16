use crate::error::Result;
use crate::runtime::AppState;
use crate::store::MessageRow;
use serde_json::json;

pub fn send(state: &AppState, tenant_id: &str, cid: &str, message: &MessageRow) -> Result<()> {
    tracing::info!(
        tenant_id,
        conversation_id = cid,
        message_id = %message.id,
        "sms fake send"
    );
    state.store.insert_event(
        tenant_id,
        Some(cid),
        "message.sent",
        json!({
            "id": message.id,
            "medium": "sms",
            "provider": "fake"
        }),
    )?;
    Ok(())
}
