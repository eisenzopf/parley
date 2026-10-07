//! UCTP host. REST-first create still sends `conversation.create` with a cid.
//! UCTP-first identity match intercepts a null cid plus visitor/cookie/E.164
//! metadata so sqlite identity can reuse a Conversation before the adapter
//! opens one.

use crate::error::Result;
use crate::runtime::AppState;
use std::net::SocketAddr;

#[cfg(not(feature = "uctp"))]
pub async fn start(_state: AppState) -> Result<SocketAddr> {
    tracing::info!("uctp feature disabled; skipping UCTP websocket bind");
    Ok(SocketAddr::from(([127, 0, 0, 1], 0)))
}

#[cfg(feature = "uctp")]
pub async fn start(state: AppState) -> Result<SocketAddr> {
    use std::sync::Arc;

    use crate::error::ApiError;
    use rvoip_auth_core::{
        AuthenticatedPrincipal, AuthenticationMethod, BearerAuthError, BearerValidator,
        ValidatedBearer,
    };
    use rvoip_core::adapter::ConnectionAdapter;
    use rvoip_core::{IdentityAssurance, IdentityId};
    use rvoip_websocket::{ConversationCreateHook, UctpWsAdapter, UctpWsConfig};
    use tokio::net::TcpListener;

    struct ParleyBearer {
        state: AppState,
    }

    #[async_trait::async_trait]
    impl BearerValidator for ParleyBearer {
        async fn validate(
            &self,
            token: &str,
        ) -> std::result::Result<IdentityAssurance, BearerAuthError> {
            self.validate_principal(token).await.map(|p| p.assurance)
        }

        async fn validate_principal(
            &self,
            token: &str,
        ) -> std::result::Result<AuthenticatedPrincipal, BearerAuthError> {
            if let Some((tenant, subject, expires)) =
                self.state
                    .store
                    .conference_principal(token)
                    .map_err(|e| BearerAuthError::Invalid(e.detail))?
            {
                return Ok(AuthenticatedPrincipal {
                    subject,
                    tenant: Some(tenant),
                    scopes: vec!["uctp:conversation-control".into()],
                    issuer: Some("parley".into()),
                    expires_at: Some(expires),
                    method: AuthenticationMethod::Bearer,
                    assurance: IdentityAssurance::UserAuthorized {
                        identity: IdentityId::new(),
                        user_id: IdentityId::new(),
                        scopes: vec!["uctp:conversation-control".into()],
                    },
                });
            }
            let ctx = crate::auth::authenticate(&self.state.store, &self.state.config, Some(token))
                .map_err(|e| BearerAuthError::Invalid(e.detail))?;
            let subject = match &ctx.actor {
                crate::auth::Actor::ApiSecret => "api".into(),
                crate::auth::Actor::Widget { visitor_id, .. } => {
                    format!("widget:{}", visitor_id.as_deref().unwrap_or("anonymous"))
                }
                crate::auth::Actor::Operator { operator_id } => format!("operator:{operator_id}"),
            };
            Ok(AuthenticatedPrincipal {
                subject,
                tenant: Some(ctx.tenant_id),
                scopes: vec!["*".into()],
                issuer: Some("parley".into()),
                expires_at: None,
                method: AuthenticationMethod::Bearer,
                assurance: IdentityAssurance::UserAuthorized {
                    identity: IdentityId::new(),
                    user_id: IdentityId::new(),
                    scopes: vec!["*".into()],
                },
            })
        }

        async fn validate_credential(
            &self,
            token: &str,
        ) -> std::result::Result<ValidatedBearer, BearerAuthError> {
            ValidatedBearer::new(self.validate_principal(token).await?, None, None)
        }
    }

    struct ParleyCreateHook {
        state: AppState,
    }

    #[async_trait::async_trait]
    impl ConversationCreateHook for ParleyCreateHook {
        async fn resolve_cid(
            &self,
            requested_cid: Option<String>,
            tenant_id: String,
            metadata: serde_json::Value,
        ) -> Option<String> {
            if let Some(cid) = requested_cid.filter(|cid| !cid.is_empty()) {
                return Some(cid);
            }
            let tenant = if tenant_id.is_empty() {
                self.state.config.tenant_id.clone()
            } else {
                tenant_id
            };
            let meta_str = |key: &str| {
                metadata
                    .get(key)
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
            };
            let keys = crate::identity::IngressKeys {
                e164: meta_str("e164"),
                visitor_id: meta_str("visitor_id"),
                cookie: meta_str("cookie"),
            };
            if keys.is_empty() {
                return None;
            }
            crate::conversation::create_or_continue(
                &self.state,
                &tenant,
                crate::conversation::CreateConversation {
                    identity: keys,
                    policy: "persistent".into(),
                    participants: Vec::new(),
                },
            )
            .await
            .ok()
            .map(|view| view.id)
        }
    }

    let listener = TcpListener::bind(&state.config.bind_uctp_ws)
        .await
        .map_err(|e| ApiError::internal(format!("bind uctp ws: {e}")))?;
    let addr = listener
        .local_addr()
        .map_err(|e| ApiError::internal(format!("uctp local_addr: {e}")))?;
    tracing::info!(%addr, "uctp websocket listening");
    if let Ok(mut bound) = state.uctp_bound.lock() {
        *bound = Some(addr);
    }
    let adapter = UctpWsAdapter::new(
        UctpWsConfig::new(
            listener,
            Arc::new(ParleyBearer {
                state: state.clone(),
            }),
        )
        .with_application_handler(Arc::new(crate::uctp_commands::Commands::new(state.clone())))
        .with_orchestrator(Arc::clone(&state.orchestrator))
        .with_conversation_create_hook(Arc::new(ParleyCreateHook {
            state: state.clone(),
        })),
    )
    .await
    .map_err(|e| ApiError::internal(format!("uctp adapter: {e}")))?;
    state
        .uctp_adapter
        .set(adapter.clone())
        .map_err(|_| ApiError::conflict("UCTP adapter already started"))?;
    state
        .orchestrator
        .register(adapter as Arc<dyn ConnectionAdapter>)
        .map_err(|e| ApiError::internal(format!("register uctp: {e}")))?;
    Ok(addr)
}

/// Admit legacy UCTP connections to a canonical core Session before consuming
/// their data events. Peer-selected IDs alone never authorize the Conversation.
#[cfg(feature = "uctp")]
pub(crate) async fn admit_legacy(
    state: &AppState,
    connection_id: &rvoip_core::ids::ConnectionId,
    principal: &rvoip_auth_core::AuthenticatedPrincipal,
) -> Result<()> {
    use crate::error::ApiError;
    use rvoip_core::ids::{ParticipantId, SessionId};
    let adapter = state
        .uctp_adapter
        .get()
        .ok_or_else(|| ApiError::internal("UCTP adapter not registered"))?;
    let (cid, wire_sid, medium) = adapter
        .inbound_context(connection_id, principal)
        .ok_or_else(|| ApiError::forbidden("UCTP route owner mismatch"))?;
    let cid = cid.ok_or_else(|| ApiError::bad_request("Conversation required"))?;
    let tenant = state.config.tenant_id.as_str();
    if principal.tenant.as_deref() != Some(tenant) || principal.issuer.as_deref() != Some("parley")
    {
        return Err(ApiError::forbidden("Conversation tenant mismatch"));
    }
    if !state.store.conference_members(tenant, &cid)?.is_empty() {
        return Err(ApiError::forbidden(
            "conference admission requires conversation-control/1",
        ));
    }
    if let Some(visitor) = principal.subject.strip_prefix("widget:") {
        let identity = state.store.lookup_identity(tenant, "visitor_id", visitor)?;
        if !identity.is_some_and(|i| i.conversation_id == cid && !i.do_not_reopen) {
            return Err(ApiError::forbidden("widget does not own Conversation"));
        }
    } else if principal.subject != "api" && !principal.subject.starts_with("operator:") {
        return Err(ApiError::forbidden("legacy admission denied"));
    }
    let customer = state
        .store
        .list_participants(tenant, &cid)?
        .into_iter()
        .find(|p| p.role == "customer")
        .ok_or_else(|| ApiError::not_found("Conversation customer missing"))?;
    let session = if let Some(existing) = state.store.get_session(tenant, &wire_sid)? {
        if existing.conversation_id != cid
            || existing.state != "active"
            || existing.medium != medium
        {
            return Err(ApiError::forbidden(
                "requested Session is not active in this Conversation and medium",
            ));
        }
        existing
    } else {
        crate::conversation::start_session(
            state,
            tenant,
            &cid,
            crate::conversation::PostSession {
                medium,
                direction: Some("inbound".into()),
            },
        )
        .await?
    };
    let sid = SessionId::from_string(session.id.clone());
    if let Err(error) = state
        .orchestrator
        .route_inbound_connection(
            connection_id.clone(),
            rvoip_core::commands::InboundAction::Accept {
                session_id: sid.clone(),
                participant_id: ParticipantId::from_string(customer.id.clone()),
            },
        )
        .await
    {
        let _ = crate::conversation::end_session(state, tenant, &session.id).await;
        return Err(ApiError::internal(format!("UCTP admission: {error}")));
    }
    state
        .store
        .insert_connection(&crate::store::ConnectionRow {
            id: connection_id.to_string(),
            tenant_id: tenant.into(),
            session_id: session.id,
            participant_id: customer.id,
            transport: "uctp-websocket".into(),
            state: "connected".into(),
        })?;
    Ok(())
}
