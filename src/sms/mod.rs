pub mod fake;
pub mod telnyx;

use crate::conversation::{self, PostMessage};
use crate::error::{ApiError, Result};
use crate::identity::IngressKeys;
use crate::runtime::AppState;
use crate::store::MessageRow;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct InboundSms {
    pub from: String,
    pub to: Option<String>,
    pub body: String,
}

pub async fn inbound(state: &AppState, msg: InboundSms) -> Result<MessageRow> {
    let tenant = state.config.tenant_id.as_str();
    let view = conversation::create_or_continue(
        state,
        tenant,
        conversation::CreateConversation {
            identity: IngressKeys {
                e164: Some(msg.from.clone()),
                visitor_id: None,
                cookie: None,
            },
            policy: "persistent".into(),
            participants: Vec::new(),
        },
    )
    .await?;
    conversation::post_message(
        state,
        tenant,
        &view.id,
        PostMessage {
            medium: "sms".into(),
            sender_participant_id: None,
            body: msg.body,
        },
        None,
    )
}

pub fn send(state: &AppState, tenant_id: &str, cid: &str, message: &MessageRow) -> Result<()> {
    let identities = state.store.identities_for_conversation(tenant_id, cid)?;
    if !identities.iter().any(|i| i.key_type == "e164") {
        crate::observe::SMS_FAIL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        return Err(ApiError::conflict(
            "outbound SMS requires an E.164 identity on the Conversation",
        ));
    }
    fake::send(state, tenant_id, cid, message)
}

pub async fn outbound(
    state: &AppState,
    tenant_id: &str,
    cid: &str,
    to: &str,
    body: &str,
) -> Result<MessageRow> {
    let identities = state.store.identities_for_conversation(tenant_id, cid)?;
    if !identities
        .iter()
        .any(|i| i.key_type == "e164" && i.key_value == to)
    {
        return Err(ApiError::forbidden("E.164 is not on this Conversation"));
    }
    conversation::post_message(
        state,
        tenant_id,
        cid,
        PostMessage {
            medium: "sms".into(),
            sender_participant_id: None,
            body: body.into(),
        },
        None,
    )
}

#[async_trait::async_trait]
pub trait SmsAdapter: Send + Sync {
    async fn send_sms(&self, to: &str, body: &str) -> Result<String>;
}
