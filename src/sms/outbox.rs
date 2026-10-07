//! Durable conference SMS delivery. Provider credentials remain server-side.
use crate::runtime::AppState;

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
        loop {
            interval.tick().await;
            let delivery = match state.store.claim_conference_delivery() {
                Ok(Some(delivery)) => delivery,
                Ok(None) => continue,
                Err(error) => {
                    tracing::error!(detail=%error.detail,"conference outbox read failed");
                    continue;
                }
            };
            let result = if state.config.vapi_chat_mode == "fake" {
                // Fake delivery acceptance is never labeled carrier delivery.
                Ok(format!("fake_{}", delivery.id))
            } else if state.telnyx.is_none()
                || state.telnyx_verifier.is_none()
                || delivery.sender_address.is_empty()
            {
                let _ = state.store.update_conference_delivery(
                    &delivery.tenant_id,
                    &delivery.id,
                    None,
                    "failed",
                    Some("Telnyx client, webhook verification key, and sender number required"),
                );
                continue;
            } else {
                tokio::time::timeout(
                    std::time::Duration::from_secs(20),
                    super::telnyx::send_live_from(
                        &state,
                        &delivery.sender_address,
                        &delivery.recipient_address,
                        &delivery.body,
                    ),
                )
                .await
                .unwrap_or_else(|_| Err(crate::ApiError::internal("provider submission timed out")))
            };
            match result {
                Ok(provider_id) if !provider_id.is_empty() => {
                    if let Err(error) = state.store.update_conference_delivery(
                        &delivery.tenant_id,
                        &delivery.id,
                        Some(&provider_id),
                        "sent",
                        None,
                    ) {
                        tracing::error!(detail=%error.detail,"conference outbox result persistence failed");
                    }
                }
                Ok(_) | Err(_) => {
                    // A timeout or missing ID does not prove non-delivery.
                    let _ = state.store.update_conference_delivery(
                        &delivery.tenant_id,
                        &delivery.id,
                        None,
                        "unknown",
                        Some("provider outcome unknown; reconcile before retry"),
                    );
                }
            }
            crate::events::publish(
                &state,
                &delivery.tenant_id,
                Some(&delivery.conversation_id),
                "message.delivery",
            );
        }
    });
}
