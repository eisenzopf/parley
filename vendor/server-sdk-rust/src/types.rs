use crate::list::as_list;
use serde::{Deserialize, Serialize};
use serde_json::Value;

fn opt_f64<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => Ok(n.as_f64()),
        Some(Value::String(s)) if s.is_empty() => Ok(None),
        Some(Value::String(s)) => s.parse().map(Some).map_err(serde::de::Error::custom),
        Some(other) => Err(serde::de::Error::custom(format!(
            "expected number or string, got {other}"
        ))),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Assistant {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub org_id: Option<String>,
    #[serde(default)]
    pub model: Option<Value>,
    #[serde(default)]
    pub voice: Option<Value>,
    #[serde(default)]
    pub transcriber: Option<Value>,
    #[serde(default)]
    pub metadata: Option<Value>,
    #[serde(default)]
    pub server_url: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(flatten)]
    pub extra: Value,
}

impl Assistant {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAssistantRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcriber: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_url: Option<String>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chat {
    pub id: String,
    #[serde(default)]
    pub assistant_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub previous_chat_id: Option<String>,
    #[serde(default)]
    pub output: Vec<ChatOutput>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(flatten)]
    pub extra: Value,
}

impl Chat {
    pub fn output_text(&self) -> String {
        let from_output = self
            .output
            .iter()
            .filter_map(ChatOutput::text)
            .collect::<Vec<_>>()
            .join("\n");
        if !from_output.is_empty() {
            return from_output;
        }
        extract_text(&self.extra)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChatOutput {
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub content: Option<Value>,
    #[serde(flatten)]
    pub extra: Value,
}

impl ChatOutput {
    pub fn text(&self) -> Option<String> {
        if let Some(text) = &self.text {
            if !text.is_empty() {
                return Some(text.clone());
            }
        }
        value_text(&self.content.clone().unwrap_or(Value::Null))
            .or_else(|| value_text(&self.extra))
    }
}

/// TypeScript `CreateChatDto`: `await client.chats.create({ input: "input" })`.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateChatRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assistant_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assistant: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_chat_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Call {
    pub id: String,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub assistant_id: Option<String>,
    #[serde(default)]
    pub squad_id: Option<String>,
    #[serde(default)]
    pub phone_number_id: Option<String>,
    #[serde(default)]
    pub customer: Option<Value>,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub ended_at: Option<String>,
    #[serde(default)]
    pub ended_reason: Option<String>,
    #[serde(default, deserialize_with = "opt_f64")]
    pub cost: Option<f64>,
    #[serde(flatten)]
    pub extra: Value,
}

impl Call {
    pub fn is_active(&self) -> bool {
        matches!(
            self.status.as_deref(),
            Some("queued" | "ringing" | "in-progress" | "forwarding")
        )
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCallRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assistant_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assistant: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone_number_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub customer: Option<Value>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListCallsRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assistant_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhoneNumber {
    pub id: String,
    #[serde(default)]
    pub number: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub assistant_id: Option<String>,
    #[serde(default)]
    pub squad_id: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePhoneNumberRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assistant_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub squad_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, rename = "type")]
    pub tool_type: Option<String>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Squad {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub members: Option<Value>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub assistant_id: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(flatten)]
    pub extra: Value,
}

fn extract_text(value: &Value) -> String {
    if let Some(text) = value_text(value) {
        return text;
    }
    as_list(value.clone())
        .iter()
        .filter_map(value_text)
        .collect::<Vec<_>>()
        .join("\n")
}

fn value_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Object(map) => {
            for key in ["text", "content", "message"] {
                if let Some(found) = map.get(key).and_then(value_text) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => {
            let joined = items.iter().filter_map(value_text).collect::<Vec<_>>().join("");
            if joined.is_empty() {
                None
            } else {
                Some(joined)
            }
        }
        _ => None,
    }
}
