use crate::config::Config;
use crate::conversation;
use crate::error::{ApiError, Result};
use crate::http;
use crate::store::{MessageRow, Store};
use axum::Router;
use chrono::Utc;
use rvoip_core::events::Event;
use rvoip_core::{Config as OrchConfig, Orchestrator};
use std::net::SocketAddr;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub store: Arc<Store>,
    pub orchestrator: Arc<Orchestrator>,
    pub sip_bound_port: Arc<std::sync::Mutex<Option<u16>>>,
    pub uctp_bound: Arc<std::sync::Mutex<Option<SocketAddr>>>,
}

pub struct App {
    pub state: AppState,
}

impl App {
    pub fn new(config: Config, store: Store) -> Result<Self> {
        let orchestrator = Orchestrator::new(OrchConfig::default());
        let app = Self {
            state: AppState {
                config: Arc::new(config),
                store: Arc::new(store),
                orchestrator,
                sip_bound_port: Arc::new(std::sync::Mutex::new(None)),
                uctp_bound: Arc::new(std::sync::Mutex::new(None)),
            },
        };
        spawn_event_mirror(app.state.clone());
        Ok(app)
    }

    pub fn router(&self) -> Router {
        http::router(self.state.clone())
    }

    pub async fn start_uctp(&self) -> Result<SocketAddr> {
        crate::uctp_host::start(self.state.clone()).await
    }

    pub fn sip_port(&self) -> Option<u16> {
        *self.state.sip_bound_port.lock().expect("sip port lock")
    }

    pub async fn serve(self) -> Result<()> {
        let addr: SocketAddr = self
            .state
            .config
            .bind_http
            .parse()
            .map_err(|e| ApiError::internal(format!("bind_http: {e}")))?;
        let uctp_state = self.state.clone();
        tokio::spawn(async move {
            match crate::uctp_host::start(uctp_state).await {
                Ok(uctp_addr) => tracing::info!(%uctp_addr, "uctp host bound"),
                Err(e) => tracing::warn!(error = %e, "uctp host did not start"),
            }
        });
        let sip_state = self.state.clone();
        tokio::spawn(async move {
            if let Err(e) = crate::sip::bind(&sip_state).await {
                tracing::warn!(error = %e, "sip bind did not start");
            }
        });
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| ApiError::internal(format!("bind http: {e}")))?;
        tracing::info!(%addr, "parley http listening");
        axum::serve(listener, self.router())
            .await
            .map_err(|e| ApiError::internal(format!("http serve: {e}")))?;
        Ok(())
    }
}

fn spawn_event_mirror(state: AppState) {
    tokio::spawn(async move {
        let mut events = state.orchestrator.subscribe_events();
        while let Ok(event) = events.recv().await {
            if let Err(err) = handle_event(&state, event) {
                tracing::debug!(error = %err, "event mirror skipped");
            }
        }
    });
}

fn handle_event(state: &AppState, event: Event) -> Result<()> {
    let tenant_id = state.config.tenant_id.as_str();
    match event {
        Event::DataMessageReceived { message, .. } => {
            let body = String::from_utf8_lossy(&message.bytes).to_string();
            let cid = state
                .store
                .list_conversations(tenant_id)?
                .into_iter()
                .find(|c| c.state == "open")
                .map(|c| c.id)
                .ok_or_else(|| ApiError::not_found("no open conversation for data message"))?;
            if state
                .store
                .list_messages(tenant_id, &cid)?
                .iter()
                .any(|m| m.body == body)
            {
                return Ok(());
            }
            let row = MessageRow {
                id: format!("msg_{}", Uuid::new_v4().simple()),
                tenant_id: tenant_id.into(),
                conversation_id: cid.clone(),
                from_participant: None,
                medium: "chat".into(),
                body: body.clone(),
                provider_id: Some(message.message_id.to_string()),
                state: "accepted".into(),
                created_at: Utc::now().to_rfc3339(),
            };
            state.store.insert_message(&row)?;
            state.store.touch_conversation(tenant_id, &cid)?;
            if state.config.ai_stub {
                let _ = conversation::post_message(
                    state,
                    tenant_id,
                    &cid,
                    conversation::PostMessage {
                        medium: "chat".into(),
                        sender_participant_id: None,
                        body: "…".into(),
                    },
                    None,
                );
            }
        }
        _ => {}
    }
    Ok(())
}
