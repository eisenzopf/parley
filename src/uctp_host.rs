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
            let ctx = crate::auth::authenticate(&self.state.store, &self.state.config, Some(token))
                .map_err(|e| BearerAuthError::Invalid(e.detail))?;
            let subject = match &ctx.actor {
                crate::auth::Actor::ApiSecret => "api".into(),
                crate::auth::Actor::Widget { visitor_id, .. } => {
                    visitor_id.clone().unwrap_or_else(|| "widget".into())
                }
                crate::auth::Actor::Operator { operator_id } => operator_id.clone(),
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
        UctpWsConfig::new(listener, Arc::new(ParleyBearer { state: state.clone() }))
            .with_orchestrator(Arc::clone(&state.orchestrator))
            .with_conversation_create_hook(Arc::new(ParleyCreateHook {
                state: state.clone(),
            })),
    )
    .await
    .map_err(|e| ApiError::internal(format!("uctp adapter: {e}")))?;
    state
        .orchestrator
        .register(adapter as Arc<dyn ConnectionAdapter>)
        .map_err(|e| ApiError::internal(format!("register uctp: {e}")))?;
    Ok(addr)
}
