use crate::error::{ApiError, Result};
use crate::runtime::AppState;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize)]
pub struct ChatReply {
    pub text: String,
    pub session_id: String,
}

#[derive(Deserialize)]
struct VapiChatResponse {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    output: Vec<VapiOutput>,
}

#[derive(Deserialize)]
struct VapiOutput {
    #[serde(default)]
    text: Option<String>,
}

pub async fn complete(state: &AppState, tenant_id: &str, cid: &str, input: &str) -> Result<ChatReply> {
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
    if state.config.vapi_api_key.is_empty() || state.config.vapi_assistant_id.is_empty() {
        return Err(ApiError::new(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "vapi-unconfigured",
            "Vapi chat is not configured",
            "set PARLEY_VAPI_API_KEY and PARLEY_VAPI_ASSISTANT_ID, or PARLEY_VAPI_CHAT=fake",
        ));
    }
    let context = timeline_prefix(state, tenant_id, cid)?;
    let client = reqwest::Client::new();
    let mut body = serde_json::json!({
        "assistantId": state.config.vapi_assistant_id,
        "input": format!("{context}{input}"),
    });
    if let Some(sid) = conv.vapi_chat_session_id {
        body["sessionId"] = serde_json::Value::String(sid);
    }
    let resp = client
        .post("https://api.vapi.ai/chat")
        .bearer_auth(&state.config.vapi_api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| ApiError::internal(format!("vapi chat: {e}")))?;
    if !resp.status().is_success() {
        return Err(ApiError::internal(format!(
            "vapi chat status {}",
            resp.status()
        )));
    }
    let parsed: VapiChatResponse = resp
        .json()
        .await
        .map_err(|e| ApiError::internal(format!("vapi chat decode: {e}")))?;
    let text = parsed
        .output
        .into_iter()
        .filter_map(|o| o.text)
        .collect::<Vec<_>>()
        .join("\n");
    let session_id = parsed.id.unwrap_or_else(|| format!("vchat_{cid}"));
    state
        .store
        .set_vapi_chat_session(tenant_id, cid, &session_id)?;
    Ok(ChatReply { text, session_id })
}

fn timeline_prefix(state: &AppState, tenant_id: &str, cid: &str) -> Result<String> {
    let messages = state.store.list_messages(tenant_id, cid)?;
    let mut buf = String::new();
    for m in messages.iter().rev().take(20).collect::<Vec<_>>().into_iter().rev() {
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
