use crate::error::ApiError;
use crate::http::Auth;
use crate::runtime::AppState;
use crate::sms::{self, InboundSms};
use crate::vapi_tools;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use serde_json::{json, Value};
use telnyx::webhooks::{unsafe_unwrap, EventPayload};

pub async fn list(
    State(_state): State<AppState>,
    Auth(_auth): Auth,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "webhooks": [] })))
}

pub async fn create(
    State(_state): State<AppState>,
    Auth(_auth): Auth,
    Json(_body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "ok": true })))
}

pub async fn vapi_tools(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let result = vapi_tools::handle(&state, &headers, body).await?;
    Ok(Json(result))
}

pub async fn sms_inbound_dev(
    State(state): State<AppState>,
    Json(body): Json<InboundSms>,
) -> Result<Json<Value>, ApiError> {
    if state.config.vapi_chat_mode != "fake" && (state.vapi.is_some() || state.telnyx.is_some()) {
        return Err(ApiError::forbidden(
            "test SMS inject is disabled when Telnyx or Vapi is live",
        ));
    }
    let row = sms::inbound(&state, body).await?;
    Ok(Json(serde_json::to_value(row).unwrap_or(json!({}))))
}

pub async fn sms_inbound(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let event = if let Some(verifier) = &state.telnyx_verifier {
        verifier
            .unwrap_headers(&headers, &body)
            .map_err(|e| ApiError::unauthorized(format!("telnyx signature: {e}")))?
    } else if state.config.vapi_chat_mode != "fake" && state.telnyx.is_some() {
        return Err(ApiError::unauthorized(
            "live Telnyx webhooks require TELNYX_PUBLIC_KEY signature verification",
        ));
    } else if let Ok(simple) = serde_json::from_slice::<InboundSms>(&body) {
        if !simple.from.is_empty() && !simple.body.is_empty() {
            let row = sms::inbound(&state, simple).await?;
            return Ok(Json(serde_json::to_value(row).unwrap_or(json!({}))));
        }
        unsafe_unwrap(&body).map_err(|e| ApiError::bad_request(format!("sms inbound: {e}")))?
    } else {
        unsafe_unwrap(&body).map_err(|e| ApiError::bad_request(format!("sms inbound: {e}")))?
    };
    // Inspect the original payload only after verification. telnyx 0.1's typed
    // Message drops autoresponse_type; carrier-handled keywords must not enter
    // task history or trigger the legacy AI reply path. Telnyx owns the actual
    // profile-wide block rule and keyword response, not this acknowledgement.
    if event.event_type.as_str() == "message.received" {
        let raw: Value = serde_json::from_slice(&body)
            .map_err(|_| ApiError::bad_request("invalid SMS event JSON"))?;
        if provider_handled_keyword(&raw) {
            return Ok(Json(
                json!({ "ok": true, "provider_handled_keyword": true }),
            ));
        }
    }
    match &event.payload {
        EventPayload::MessageSent(msg) | EventPayload::MessageFinalized(msg) => {
            crate::sms::telnyx::apply_status(&state, msg.clone())?;
            return Ok(Json(json!({ "ok": true, "outbound": true })));
        }
        _ => {}
    }
    let inbound = match event.payload {
        EventPayload::MessageReceived(msg) => {
            if msg
                .direction
                .as_ref()
                .is_some_and(|d| d.as_str() == "outbound")
            {
                return Ok(Json(json!({ "ok": true, "ignored": true })));
            }
            if let (Some(provider_id), Some(remote), Some(local), Some(text)) = (
                msg.id.as_deref(),
                msg.from.as_ref().and_then(|f| f.phone_number.as_deref()),
                msg.to
                    .as_ref()
                    .and_then(|tos| tos.first())
                    .and_then(|t| t.phone_number.as_deref()),
                msg.text.as_deref(),
            ) {
                match state.store.receive_conference_sms(
                    &state.config.tenant_id,
                    provider_id,
                    remote,
                    local,
                    text,
                )? {
                    crate::store::conference::InboundSmsOutcome::Routed(row) => {
                        crate::events::publish(
                            &state,
                            &state.config.tenant_id,
                            Some(&row.conversation_id),
                            "message.received",
                        );
                        return Ok(Json(
                            json!({ "ok": true, "message_id": row.id, "conversation_id": row.conversation_id }),
                        ));
                    }
                    crate::store::conference::InboundSmsOutcome::Held { id } => {
                        return Ok(Json(json!({"ok":true,"state":"held","inbox_id":id})));
                    }
                    crate::store::conference::InboundSmsOutcome::Unmatched => {}
                }
            }
            InboundSms {
                from: msg.from.and_then(|f| f.phone_number).unwrap_or_default(),
                to: msg
                    .to
                    .and_then(|tos| tos.into_iter().next())
                    .and_then(|t| t.phone_number),
                body: msg.text.unwrap_or_default(),
            }
        }
        EventPayload::Other(raw) => {
            if !looks_like_inbound_sms(&event.event_type, &raw) {
                return Ok(Json(json!({ "ok": true, "ignored": true })));
            }
            inbound_from_payload(&raw)
                .ok_or_else(|| ApiError::bad_request("inbound SMS payload missing from/text"))?
        }
        _ => {
            return Ok(Json(json!({ "ok": true, "ignored": true })));
        }
    };
    if inbound.from.is_empty() || inbound.body.trim().is_empty() {
        return Err(ApiError::bad_request("inbound SMS missing from or body"));
    }
    tracing::info!(from = %inbound.from, "sms inbound");
    let row = sms::inbound(&state, inbound).await?;
    Ok(Json(serde_json::to_value(row).unwrap_or(json!({}))))
}

fn provider_handled_keyword(raw: &Value) -> bool {
    let payload = raw.pointer("/data/payload").unwrap_or(raw);
    if payload
        .get("direction")
        .and_then(Value::as_str)
        .is_some_and(|d| d.eq_ignore_ascii_case("outbound"))
    {
        return false;
    }
    if payload
        .get("autoresponse_type")
        .and_then(Value::as_str)
        .is_some_and(|kind| !kind.trim().is_empty())
    {
        return true;
    }
    // Defaults remain reserved even when no custom profile response exists.
    // Match whole messages only; "help with pickup" remains a normal task reply.
    matches!(
        payload
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_ascii_uppercase()
            .as_str(),
        "STOP"
            | "STOPALL"
            | "STOP ALL"
            | "UNSUBSCRIBE"
            | "CANCEL"
            | "END"
            | "QUIT"
            | "START"
            | "UNSTOP"
            | "HELP"
    )
}

fn looks_like_inbound_sms(event_type: &telnyx::webhooks::EventType, raw: &Value) -> bool {
    if event_type.as_str() == "message.received" {
        return true;
    }
    inbound_from_payload(raw).is_some()
        && raw
            .get("direction")
            .or_else(|| raw.pointer("/payload/direction"))
            .and_then(|d| d.as_str())
            .is_some_and(|d| d.eq_ignore_ascii_case("inbound"))
}

fn inbound_from_payload(raw: &Value) -> Option<InboundSms> {
    let payload = raw.get("payload").unwrap_or(raw);
    if payload
        .get("direction")
        .and_then(|d| d.as_str())
        .is_some_and(|d| d.eq_ignore_ascii_case("outbound"))
    {
        return None;
    }
    let from = payload
        .pointer("/from/phone_number")
        .or_else(|| payload.pointer("/from"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let to = payload
        .pointer("/to/0/phone_number")
        .or_else(|| payload.pointer("/to/phone_number"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let body = payload
        .get("text")
        .or_else(|| payload.get("body"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if from.is_empty() || body.trim().is_empty() {
        return None;
    }
    Some(InboundSms { from, to, body })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inbound_from_telnyx_payload_object() {
        let raw = json!({
            "direction": "inbound",
            "from": { "phone_number": "+14155550123" },
            "to": [{ "phone_number": "+18058253932" }],
            "text": "hello from sms"
        });
        let got = inbound_from_payload(&raw).expect("inbound");
        assert_eq!(got.from, "+14155550123");
        assert_eq!(got.body, "hello from sms");
    }

    #[test]
    fn outbound_payload_is_ignored() {
        let raw = json!({
            "direction": "outbound",
            "from": { "phone_number": "+18058253932" },
            "text": "ai reply"
        });
        assert!(inbound_from_payload(&raw).is_none());
    }
}
