//! SIP ingress policy. Bind helpers live behind the `sip` feature; identity
//! match and the one-live-voice-session rule are product code here.

use crate::conversation::{self, PostSession};
#[cfg(feature = "sip")]
use crate::error::ApiError;
use crate::error::Result;
use crate::identity::IngressKeys;
use crate::runtime::AppState;
use chrono::Utc;
use serde::Serialize;

#[derive(Clone, Debug)]
pub struct Invite {
    pub cli_e164: String,
    pub did: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "disposition", rename_all = "snake_case")]
pub enum Admit {
    Accepted {
        conversation_id: String,
        session_id: String,
    },
    Busy {
        conversation_id: String,
    },
    Closed {
        conversation_id: String,
    },
    Voicemail {
        conversation_id: String,
        message_id: String,
    },
}

/// Pull an E.164 CLI from a SIP `From` header or URI.
pub fn parse_cli_from_sip(from: &str) -> Option<String> {
    let uri = extract_uri(from)?;
    let user = uri_user(&uri)?;
    normalize_e164(&user)
}

fn extract_uri(from: &str) -> Option<String> {
    let trimmed = from.trim();
    if let Some(start) = trimmed.find('<') {
        let inner = trimmed.get(start + 1..)?;
        let end = inner.find('>')?;
        return Some(inner[..end].trim().to_string());
    }
    Some(trimmed.split(';').next()?.trim().to_string())
}

fn uri_user(uri: &str) -> Option<String> {
    let rest = uri
        .strip_prefix("sips:")
        .or_else(|| uri.strip_prefix("sip:"))
        .or_else(|| uri.strip_prefix("tel:"))
        .unwrap_or(uri);
    let user = rest.split('@').next()?.trim();
    if user.is_empty() {
        None
    } else {
        Some(user.to_string())
    }
}

fn normalize_e164(user: &str) -> Option<String> {
    let decoded = user.replace("%2B", "+").replace("%2b", "+");
    let digits: String = decoded.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() < 8 {
        return None;
    }
    Some(format!("+{digits}"))
}

pub async fn admit_invite(state: &AppState, invite: Invite) -> Result<Admit> {
    let tenant = state.config.tenant_id.as_str();
    let keys = IngressKeys {
        e164: Some(invite.cli_e164.clone()),
        visitor_id: None,
        cookie: None,
    };
    let created = conversation::create_or_continue(
        state,
        tenant,
        conversation::CreateConversation {
            identity: keys,
            policy: "persistent".into(),
            participants: Vec::new(),
        },
    )
    .await?;
    if created.state != "open" {
        return Ok(Admit::Closed {
            conversation_id: created.id,
        });
    }
    if state.store.count_live_voice_sessions(tenant, &created.id)? > 0 {
        let _ = crate::sms::send(
            state,
            tenant,
            &created.id,
            &crate::store::MessageRow {
                id: "busy".into(),
                tenant_id: tenant.into(),
                conversation_id: created.id.clone(),
                from_participant: None,
                medium: "sms".into(),
                body: "We're already on a live voice session.".into(),
                provider_id: None,
                state: "queued".into(),
                created_at: Utc::now().to_rfc3339(),
            },
        );
        return Ok(Admit::Busy {
            conversation_id: created.id,
        });
    }
    let open = {
        let hours = state.hours.lock().expect("hours");
        crate::hours::is_open_config(&hours, Utc::now())
    };
    if !open {
        let msg = conversation::leave_voicemail(state, tenant, &created.id)?;
        return Ok(Admit::Voicemail {
            conversation_id: created.id,
            message_id: msg.id,
        });
    }
    let session = conversation::start_session(
        state,
        tenant,
        &created.id,
        PostSession {
            medium: "voice".into(),
            direction: Some("inbound".into()),
        },
    )
    .await?;
    Ok(Admit::Accepted {
        conversation_id: created.id,
        session_id: session.id,
    })
}

pub async fn bind(state: &AppState) -> Result<()> {
    #[cfg(not(feature = "sip"))]
    {
        let _ = state;
        tracing::info!("sip feature disabled; ingress policy still available via admit_invite");
        return Ok(());
    }
    #[cfg(feature = "sip")]
    {
        bind_sip_ua(state).await
    }
}

#[cfg(feature = "sip")]
async fn bind_sip_ua(state: &AppState) -> Result<()> {
    use rvoip_sip::{Config as SipConfig, Endpoint, EndpointProfile};

    let port = state
        .config
        .bind_sip
        .rsplit(':')
        .next()
        .and_then(|p| p.parse().ok())
        .unwrap_or(5060);
    let mut sip_cfg = SipConfig::local("parley", port);
    sip_cfg.media_port_start = 42000;
    sip_cfg.media_port_end = 42100;
    let mut endpoint = Endpoint::builder()
        .name("parley")
        .profile(EndpointProfile::Custom(sip_cfg))
        .build()
        .await
        .map_err(|e| ApiError::internal(format!("sip endpoint: {e}")))?;
    *state.sip_bound_port.lock().expect("sip port lock") = Some(port);

    // Endpoint owns the SIP control stream (same pattern as rvoip local-call).
    // SipAdapter is not registered: it claims that stream and drops From/CLI.
    let ingress = state.clone();
    tokio::spawn(async move {
        loop {
            match endpoint.wait_for_incoming().await {
                Ok(incoming) => {
                    if let Err(err) = handle_incoming_invite(&ingress, incoming).await {
                        tracing::warn!(error = %err, "sip invite handling failed");
                    }
                }
                Err(err) => {
                    tracing::warn!(error = %err, "sip incoming wait ended");
                    break;
                }
            }
        }
    });

    tracing::info!(port, "sip ua listening");
    Ok(())
}

#[cfg(feature = "sip")]
async fn handle_incoming_invite(
    state: &AppState,
    incoming: rvoip_sip::EndpointIncomingCall,
) -> Result<()> {
    let from = incoming.from().to_string();
    let to = incoming.to().to_string();
    tracing::info!(%from, %to, "sip invite");
    let Some(cli) = parse_cli_from_sip(&from) else {
        let _ = incoming.reject(488, "Not Acceptable").await;
        return Err(ApiError::bad_request("sip invite had no parseable CLI"));
    };
    let did = parse_cli_from_sip(&to);
    match admit_invite(state, Invite { cli_e164: cli, did }).await {
        Err(err) => {
            let _ = incoming.reject(500, "Server Error").await;
            Err(err)
        }
        Ok(Admit::Accepted {
            conversation_id,
            session_id,
        }) => {
            let tenant = state.config.tenant_id.as_str();
            let customer = state
                .store
                .list_participants(tenant, &conversation_id)?
                .into_iter()
                .find(|p| p.role == "customer")
                .ok_or_else(|| ApiError::not_found("customer participant"))?;
            persist_sip_connection(
                state,
                tenant,
                &session_id,
                &customer.id,
                &format!("conn_sip_{}", incoming.id()),
            )?;
            let call = incoming
                .answer()
                .await
                .map_err(|e| ApiError::internal(format!("sip accept: {e}")))?;
            if let Ok(ids) = crate::vapi_voice::ai_and_customer(state, tenant, &conversation_id) {
                tracing::info!(
                    cid = %conversation_id,
                    ai = %ids.ai_participant_id,
                    customer = %ids.customer_participant_id,
                    "sip voice session ready; vapi attaches on orchestrator WebRTC Talk"
                );
            }
            let ended = state.clone();
            let sid = session_id.clone();
            tokio::spawn(async move {
                let _ = call.wait_for_end(None).await;
                let tenant = ended.config.tenant_id.as_str();
                let _ = conversation::end_session(&ended, tenant, &sid).await;
            });
            Ok(())
        }
        Ok(Admit::Busy { .. }) => {
            let _ = incoming.busy().await;
            Ok(())
        }
        Ok(Admit::Closed { .. }) => {
            let _ = incoming.decline().await;
            Ok(())
        }
        Ok(Admit::Voicemail { .. }) => {
            let _ = incoming.reject(480, "Temporarily Unavailable").await;
            Ok(())
        }
    }
}

#[cfg(feature = "sip")]
fn persist_sip_connection(
    state: &AppState,
    tenant: &str,
    session_id: &str,
    participant_id: &str,
    connection_id: &str,
) -> Result<()> {
    use crate::store::ConnectionRow;

    let synthetic = format!("conn_customer_{session_id}");
    let _ = state.store.delete_connection(tenant, &synthetic);
    state.store.insert_connection(&ConnectionRow {
        id: connection_id.into(),
        tenant_id: tenant.into(),
        session_id: session_id.into(),
        participant_id: participant_id.into(),
        transport: "sip".into(),
        state: "connected".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cli_from_name_addr() {
        assert_eq!(
            parse_cli_from_sip(r#""Alice" <sip:+14155550111@127.0.0.1:5070>;tag=x"#).as_deref(),
            Some("+14155550111")
        );
        assert_eq!(
            parse_cli_from_sip("sip:+14155550111@127.0.0.1").as_deref(),
            Some("+14155550111")
        );
        assert_eq!(
            parse_cli_from_sip(r#"User <sip:%2B14155550111@127.0.0.1:51949>;tag=x"#).as_deref(),
            Some("+14155550111")
        );
        assert_eq!(parse_cli_from_sip("sip:alice@127.0.0.1"), None);
    }
}
