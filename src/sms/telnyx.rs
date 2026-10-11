//! Telnyx SMS via crates.io `telnyx`. Inbound is signature-checked when a public key is set.

use crate::error::{ApiError, Result};
use crate::runtime::AppState;
use crate::store::MessageRow;
use telnyx::messaging::{Message, SendMessageParams};

pub fn inbound_path() -> &'static str {
    "/v1/sms/inbound"
}

pub async fn send_live(state: &AppState, to: &str, body: &str) -> Result<String> {
    send_live_from(state, &state.config.telnyx_from, to, body).await
}

pub async fn send_live_from(state: &AppState, from: &str, to: &str, body: &str) -> Result<String> {
    super::enrollment::authorize(&state.config, from, to, body)?;
    if from.is_empty() {
        return Err(ApiError::bad_request(
            "configured SMS sender number required",
        ));
    }
    let client = state.telnyx.as_ref().ok_or_else(|| {
        ApiError::new(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "telnyx-unconfigured",
            "Telnyx is not configured",
            "set TELNYX_TEST_API_KEY",
        )
    })?;
    if to == from {
        return Err(ApiError::conflict(
            "refusing to send SMS to the lab number itself",
        ));
    }
    let mut params = SendMessageParams::sms(to, from, body);
    if !state.config.telnyx_messaging_profile_id.is_empty() {
        params.messaging_profile_id = Some(state.config.telnyx_messaging_profile_id.clone());
    }
    let sent = client
        .messages()
        .send(params)
        .await
        .map_err(|e| {
            // Keep provider diagnostics available without logging message bodies,
            // destinations, credentials, or response snippets.
            tracing::warn!(status = ?e.status(), request_id = ?e.request_id(),
                codes = ?e.api_errors().iter().map(|error| &error.code).collect::<Vec<_>>(),
                "Telnyx SMS submission failed");
            ApiError::internal(format!("telnyx send: {e}"))
        })?;
    if let Some(reason) = delivery_problem(&sent) {
        return Err(ApiError::internal(format!("telnyx delivery: {reason}")));
    }
    Ok(sent.id.unwrap_or_default())
}

pub fn queue_outbound(
    state: &AppState,
    tenant_id: &str,
    cid: &str,
    message: &MessageRow,
    to: &str,
) {
    let state = state.clone();
    let tenant_id = tenant_id.to_string();
    let cid = cid.to_string();
    let to = to.to_string();
    let body = message.body.clone();
    let mid = message.id.clone();
    tokio::spawn(async move {
        match send_live(&state, &to, &body).await {
            Ok(provider_id) => {
                let _ = state.store.set_message_delivery(
                    &tenant_id,
                    &mid,
                    Some(provider_id.as_str()).filter(|s| !s.is_empty()),
                    "sent",
                );
                let _ = crate::events::emit(
                    &state,
                    &tenant_id,
                    Some(&cid),
                    "message.sent",
                    serde_json::json!({
                        "id": mid,
                        "medium": "sms",
                        "provider": "telnyx",
                        "provider_id": provider_id,
                        "to": to,
                    }),
                );
            }
            Err(e) => {
                crate::observe::SMS_FAIL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let _ = state
                    .store
                    .set_message_delivery(&tenant_id, &mid, None, "delivery_failed");
                let _ = crate::events::emit(
                    &state,
                    &tenant_id,
                    Some(&cid),
                    "message.failed",
                    serde_json::json!({
                        "id": mid,
                        "medium": "sms",
                        "error": e.to_string(),
                    }),
                );
                tracing::warn!(error = %e, to = %to, "telnyx outbound failed");
            }
        }
    });
}

pub fn apply_status(state: &AppState, msg: Message) -> Result<()> {
    let Some(provider_id) = msg.id.clone() else {
        return Ok(());
    };
    let tenant = state.config.tenant_id.as_str();
    let failure = delivery_problem(&msg);
    let delivered = msg.to.iter().flatten().any(|to| {
        to.status
            .as_ref()
            .is_some_and(|s| s.as_str() == "delivered")
    });
    let status = if failure.is_some() {
        "failed"
    } else if delivered {
        "delivered"
    } else {
        "sent"
    };
    state
        .store
        .record_conference_receipt(tenant, &provider_id, status, failure.as_deref())?;
    if let Some(delivery) = state
        .store
        .conference_delivery_for_provider(tenant, &provider_id)?
    {
        state.store.update_conference_delivery(
            tenant,
            &delivery.id,
            Some(&provider_id),
            status,
            failure.as_deref(),
        )?;
        crate::events::publish(
            state,
            tenant,
            Some(&delivery.conversation_id),
            "message.delivery",
        );
        return Ok(());
    }
    let Some(row) = state.store.find_message_by_provider(tenant, &provider_id)? else {
        return Ok(());
    };
    if let Some(reason) = delivery_problem(&msg) {
        crate::observe::SMS_FAIL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        state
            .store
            .set_message_delivery(tenant, &row.id, Some(&provider_id), "delivery_failed")?;
        crate::events::emit(
            state,
            tenant,
            Some(&row.conversation_id),
            "message.failed",
            serde_json::json!({
                "id": row.id,
                "medium": "sms",
                "provider": "telnyx",
                "provider_id": provider_id,
                "error": reason,
            }),
        )?;
        tracing::warn!(provider_id = %provider_id, error = %reason, "telnyx delivery failed");
    }
    Ok(())
}

fn delivery_problem(msg: &Message) -> Option<String> {
    if let Some(errors) = &msg.errors {
        if let Some(err) = errors.first() {
            return Some(err.title.clone().unwrap_or_else(|| "telnyx error".into()));
        }
    }
    for dest in msg.to.iter().flatten() {
        let status = dest.status.as_ref().map(|s| s.as_str()).unwrap_or_default();
        if status.contains("fail") || status == "expired" {
            return Some(status.to_string());
        }
    }
    None
}
