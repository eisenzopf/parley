use crate::error::{ApiError, Result};
use crate::runtime::AppState;
use crate::store::MessageRow;
use vapi::CreateChatRequest;

#[derive(Clone, Debug, serde::Serialize)]
pub struct ChatReply {
    pub text: String,
    pub session_id: String,
}

pub fn reply_enabled(state: &AppState) -> bool {
    state.config.vapi_chat_mode == "fake"
        || (state.config.vapi_configured() && !state.config.vapi_assistant_id.is_empty())
}

pub fn should_reply(state: &AppState, row: &MessageRow) -> bool {
    if !reply_enabled(state) {
        return false;
    }
    if row.medium != "chat" && row.medium != "sms" {
        return false;
    }
    if let Some(from) = &row.from_participant {
        if let Ok(parts) = state
            .store
            .list_participants(&row.tenant_id, &row.conversation_id)
        {
            if parts.iter().any(|p| p.id == *from && p.kind == "ai") {
                return false;
            }
        }
    }
    true
}

/// Persist an AI Message. Failures are logged; inbound persist already succeeded.
pub async fn reply_after(state: &AppState, row: &MessageRow) {
    if !should_reply(state, row) {
        return;
    }
    if let Err(e) =
        reply_to_customer(state, &row.tenant_id, &row.conversation_id, &row.medium).await
    {
        tracing::warn!(error = %e, cid = %row.conversation_id, "ai reply skipped");
    }
}

pub fn spawn_reply(state: AppState, row: MessageRow) {
    if !should_reply(&state, &row) {
        return;
    }
    tokio::spawn(async move {
        reply_after(&state, &row).await;
    });
}

pub async fn reply_to_customer(
    state: &AppState,
    tenant_id: &str,
    cid: &str,
    medium: &str,
) -> Result<MessageRow> {
    let input = state
        .store
        .list_messages(tenant_id, cid)?
        .into_iter()
        .rev()
        .find(|m| {
            m.medium == medium
                && m.from_participant
                    .as_ref()
                    .map(|id| {
                        state
                            .store
                            .list_participants(tenant_id, cid)
                            .ok()
                            .and_then(|ps| {
                                ps.into_iter().find(|p| p.id == *id).map(|p| p.kind != "ai")
                            })
                            .unwrap_or(true)
                    })
                    .unwrap_or(true)
        })
        .map(|m| m.body)
        .unwrap_or_default();
    let reply = complete(state, tenant_id, cid, &input).await?;
    let text = customer_facing_text(&reply.text).unwrap_or_else(|| {
        if medium == "sms" {
            "Got your message. I'll follow up shortly.".into()
        } else {
            String::new()
        }
    });
    if text.is_empty() {
        return Err(ApiError::internal("empty vapi chat reply"));
    }
    let text = if medium == "sms" {
        truncate_sms(&text)
    } else {
        text
    };
    let ai = state
        .store
        .list_participants(tenant_id, cid)?
        .into_iter()
        .find(|p| p.kind == "ai")
        .map(|p| p.id);
    crate::conversation::post_message(
        state,
        tenant_id,
        cid,
        crate::conversation::PostMessage {
            medium: medium.into(),
            sender_participant_id: ai,
            body: text,
            inbound: false,
        },
        None,
    )
}

pub async fn complete(
    state: &AppState,
    tenant_id: &str,
    cid: &str,
    input: &str,
) -> Result<ChatReply> {
    let conv = state
        .store
        .get_conversation(tenant_id, cid)?
        .ok_or_else(|| ApiError::not_found("conversation not found"))?;
    if state.config.vapi_chat_mode == "fake" {
        let session_id = conv
            .vapi_chat_session_id
            .unwrap_or_else(|| format!("vchat_{cid}"));
        state
            .store
            .set_vapi_chat_session(tenant_id, cid, &session_id)?;
        return Ok(ChatReply {
            text: format!("fake-reply:{input}"),
            session_id,
        });
    }
    if !state.config.vapi_configured() || state.config.vapi_assistant_id.is_empty() {
        return Err(ApiError::new(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "vapi-unconfigured",
            "Vapi chat is not configured",
            "set VAPI_PRIVATE_KEY; first boot provisions the Parley assistant",
        ));
    }
    let client = state.vapi.as_ref().ok_or_else(|| {
        ApiError::new(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "vapi-unconfigured",
            "Vapi chat is not configured",
            "Vapi client failed to start",
        )
    })?;
    let context = timeline_prefix(state, tenant_id, cid)?;
    let previous = conv.vapi_chat_session_id.clone();
    let mut body = CreateChatRequest {
        assistant_id: Some(state.config.vapi_assistant_id.clone()),
        input: Some(format!("{context}{input}")),
        extra: serde_json::json!({
            "assistantOverrides": {
                "model": {
                    "provider": "openai",
                    "model": "gpt-4o-mini",
                    "messages": [{
                        "role": "system",
                        "content": "You are Parley. Reply in short plain text. Never call tools."
                    }],
                    "tools": []
                }
            }
        }),
        ..Default::default()
    };
    if let Some(sid) = previous.clone() {
        body.previous_chat_id = Some(sid);
    }
    let chat = match client.chats().create(body).await {
        Ok(chat) => chat,
        Err(e) => {
            tracing::warn!(error = %e, "vapi chat with overrides failed");
            let mut fallback = CreateChatRequest {
                assistant_id: Some(state.config.vapi_assistant_id.clone()),
                input: Some(format!(
                    "Reply in one or two short sentences of plain text. Do not call tools.\n{context}{input}"
                )),
                ..Default::default()
            };
            if let Some(sid) = previous {
                fallback.previous_chat_id = Some(sid);
            }
            client
                .chats()
                .create(fallback)
                .await
                .map_err(|e| ApiError::internal(format!("vapi chat: {e}")))?
        }
    };
    let mut text = chat.output_text();
    if customer_facing_text(&text).is_none() {
        let retry = CreateChatRequest {
            assistant_id: Some(state.config.vapi_assistant_id.clone()),
            input: Some(format!(
                "Reply in one or two short sentences of plain text. Do not call tools.\n{input}"
            )),
            extra: serde_json::json!({
                "assistantOverrides": {
                    "model": {
                        "provider": "openai",
                        "model": "gpt-4o-mini"
                    }
                }
            }),
            ..Default::default()
        };
        match client.chats().create(retry).await {
            Ok(second) => text = second.output_text(),
            Err(e) => tracing::warn!(error = %e, "vapi chat retry skipped"),
        }
    }
    let session_id = chat
        .previous_chat_id
        .clone()
        .or(chat.session_id.clone())
        .unwrap_or(chat.id.clone());
    state
        .store
        .set_vapi_chat_session(tenant_id, cid, &chat.id)?;
    Ok(ChatReply { text, session_id })
}

fn customer_facing_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("no result returned")
        || lower.contains("docs.vapi.ai")
        || lower == "one moment"
        || lower.starts_with("one moment\n")
    {
        return None;
    }
    Some(trimmed.to_string())
}

fn truncate_sms(text: &str) -> String {
    const MAX: usize = 1400;
    if text.chars().count() <= MAX {
        return text.to_string();
    }
    text.chars().take(MAX - 1).collect::<String>() + "…"
}

fn timeline_prefix(state: &AppState, tenant_id: &str, cid: &str) -> Result<String> {
    let messages = state.store.list_messages(tenant_id, cid)?;
    let mut buf = String::new();
    for m in messages
        .iter()
        .rev()
        .take(20)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        buf.push_str(&format!("{}: {}\n", m.medium, m.body));
        if buf.len() > 8000 {
            break;
        }
    }
    if buf.is_empty() {
        Ok(String::new())
    } else {
        Ok(format!("Prior conversation:\n{buf}\n---\n"))
    }
}
