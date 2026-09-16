use crate::error::{ApiError, Result};
use crate::pickup;
use crate::runtime::AppState;
use axum::http::HeaderMap;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
pub struct ToolRequest {
    #[serde(default)]
    pub message: Option<ToolMessage>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub parameters: Option<Value>,
}

#[derive(Deserialize)]
pub struct ToolMessage {
    #[serde(default)]
    pub tool_call_list: Vec<ToolCall>,
    #[serde(default, rename = "toolCallList")]
    pub tool_call_list_camel: Vec<ToolCall>,
}

#[derive(Deserialize)]
pub struct ToolCall {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub parameters: Option<Value>,
}

pub async fn handle(state: &AppState, headers: &HeaderMap, body: Value) -> Result<Value> {
    authorize_vapi(state, headers)?;
    let req: ToolRequest = serde_json::from_value(body.clone()).unwrap_or(ToolRequest {
        message: None,
        tool_call_id: None,
        name: None,
        parameters: None,
    });
    let calls = req
        .message
        .as_ref()
        .map(|m| {
            if !m.tool_call_list.is_empty() {
                m.tool_call_list.as_slice()
            } else {
                m.tool_call_list_camel.as_slice()
            }
        })
        .unwrap_or(&[]);
    if calls.is_empty() {
        if let Some(name) = req.name.as_deref() {
            let result = dispatch(state, name, req.parameters.as_ref().unwrap_or(&json!({}))).await?;
            return Ok(json!({ "results": [{ "toolCallId": req.tool_call_id, "result": result }] }));
        }
        return Ok(json!({ "results": [] }));
    }
    let mut results = Vec::new();
    for call in calls {
        let name = call.name.as_deref().unwrap_or("");
        let params = call.parameters.clone().unwrap_or_else(|| json!({}));
        let result = dispatch(state, name, &params).await?;
        results.push(json!({
            "toolCallId": call.id,
            "result": result
        }));
    }
    Ok(json!({ "results": results }))
}

fn authorize_vapi(state: &AppState, headers: &HeaderMap) -> Result<()> {
    if state.config.vapi_api_key.is_empty() {
        return Ok(());
    }
    let presented = headers
        .get("x-vapi-secret")
        .or_else(|| headers.get(axum::http::header::AUTHORIZATION))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let presented = presented.strip_prefix("Bearer ").unwrap_or(presented);
    if presented != state.config.vapi_api_key {
        return Err(ApiError::unauthorized("invalid vapi tool secret"));
    }
    Ok(())
}

async fn dispatch(state: &AppState, name: &str, params: &Value) -> Result<Value> {
    let cid = params
        .get("conversation_id")
        .or_else(|| params.get("cid"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let tenant_id = &state.config.tenant_id;
    match name {
        "request_human" => {
            if cid.is_empty() {
                return Err(ApiError::bad_request("conversation_id required"));
            }
            let sid = params.get("session_id").and_then(Value::as_str);
            pickup::request(state, tenant_id, cid, sid).await?;
            crate::observe::pickup();
            Ok(json!({ "ok": true, "pickup": "requested" }))
        }
        "send_message" => {
            let to = params.get("to").and_then(Value::as_str).unwrap_or("");
            if cid.is_empty() {
                return Err(ApiError::bad_request("conversation_id required"));
            }
            if to.is_empty() {
                return Err(ApiError::bad_request("to E.164 required"));
            }
            let body = params
                .get("body")
                .and_then(Value::as_str)
                .ok_or_else(|| ApiError::bad_request("body required"))?;
            let msg = crate::sms::outbound(state, tenant_id, cid, to, body).await?;
            Ok(json!({ "ok": true, "message_id": msg.id }))
        }
        "close_session" => {
            let sid = params
                .get("session_id")
                .and_then(Value::as_str)
                .ok_or_else(|| ApiError::bad_request("session_id required"))?;
            crate::conversation::end_session(state, tenant_id, sid).await?;
            Ok(json!({ "ok": true }))
        }
        other => Err(ApiError::bad_request(format!("unknown tool {other}"))),
    }
}
