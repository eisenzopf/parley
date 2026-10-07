use crate::config::{Config, WeeklyHours};
use crate::error::{ApiError, Result};
use crate::events::LiveEvent;
use crate::http;
use crate::store::{MessageRow, Store};
use axum::Router;
use chrono::Utc;
use rvoip_core::events::Event;
use rvoip_core::{Config as OrchConfig, Orchestrator};
#[cfg(feature = "vapi")]
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use telnyx::webhooks::Verifier;
use tokio::sync::broadcast;
use uuid::Uuid;
use vapi::VapiClient;

#[derive(Clone, Debug)]
pub struct LiveBindings {
    pub public_base: String,
    pub uctp_ws_url: String,
}

impl LiveBindings {
    pub fn from_config(config: &Config) -> Self {
        Self {
            public_base: config.public_http_base(),
            uctp_ws_url: config.public_uctp_ws(),
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    /// Serialize speaking-peer commits with explicit voice teardown.
    pub conference_handoff: Arc<tokio::sync::Mutex<()>>,
    #[cfg(feature = "uctp")]
    pub uctp_adapter: Arc<std::sync::OnceLock<Arc<rvoip_websocket::UctpWsAdapter>>>,
    #[cfg(feature = "media-webrtc")]
    pub conference_browser: Arc<rvoip_webrtc::WebRtcAdapter>,
    pub config: Arc<Config>,
    pub store: Arc<Store>,
    pub orchestrator: Arc<Orchestrator>,
    pub sip_bound_port: Arc<std::sync::Mutex<Option<u16>>>,
    pub uctp_bound: Arc<std::sync::Mutex<Option<SocketAddr>>>,
    pub vapi: Option<Arc<VapiClient>>,
    pub telnyx: Option<Arc<telnyx::Client>>,
    pub telnyx_verifier: Option<Arc<Verifier>>,
    pub live: Arc<LiveBindings>,
    pub live_events: broadcast::Sender<LiveEvent>,
    pub hours: Arc<Mutex<WeeklyHours>>,
    #[cfg(feature = "vapi")]
    pub vapi_adapter: Option<Arc<rvoip_vapi::VapiAdapter>>,
    /// Session id → Vapi connection id for mute / unmute.
    #[cfg(feature = "vapi")]
    pub vapi_calls: Arc<Mutex<HashMap<String, rvoip_core::ids::ConnectionId>>>,
}

pub struct App {
    pub state: AppState,
}

impl App {
    pub fn new(config: Config, store: Store) -> Result<Self> {
        Self::build(
            config,
            store,
            #[cfg(feature = "vapi")]
            None,
        )
    }

    /// Use an explicitly configured voice adapter (including loopback provider
    /// fixtures) while keeping the same event admission and attachment path.
    #[cfg(feature = "vapi")]
    pub fn with_voice_adapter(
        config: Config,
        store: Store,
        adapter: Arc<rvoip_vapi::VapiAdapter>,
    ) -> Result<Self> {
        Self::build(config, store, Some(adapter))
    }

    fn build(
        config: Config,
        store: Store,
        #[cfg(feature = "vapi")] voice_adapter: Option<Arc<rvoip_vapi::VapiAdapter>>,
    ) -> Result<Self> {
        crate::tls::ensure_rustls_ring();
        config.conference_network.validate()?;
        let orchestrator = Orchestrator::new(OrchConfig::default());
        #[cfg(feature = "media-webrtc")]
        let conference_browser = {
            let media = config.conference_network.webrtc_config()?;
            let adapter = rvoip_webrtc::WebRtcAdapter::new(media);
            orchestrator
                .register(adapter.clone())
                .map_err(|e| ApiError::internal(format!("conference WebRTC: {e}")))?;
            adapter
        };
        let live_providers = config.vapi_chat_mode != "fake";
        let vapi = if live_providers && config.vapi_configured() {
            Some(Arc::new(
                VapiClient::new(config.vapi_api_key.clone())
                    .map_err(|e| ApiError::internal(format!("vapi client: {e}")))?,
            ))
        } else {
            None
        };
        let telnyx = if live_providers && config.telnyx_configured() {
            Some(Arc::new(
                telnyx::Client::builder()
                    .api_key(config.telnyx_api_key.clone())
                    .build()
                    .map_err(|e| ApiError::internal(format!("telnyx client: {e}")))?,
            ))
        } else {
            None
        };
        let telnyx_verifier = if live_providers && !config.telnyx_public_key.is_empty() {
            Some(Arc::new(Verifier::new(&config.telnyx_public_key).map_err(
                |e| ApiError::internal(format!("telnyx webhook verifier: {e}")),
            )?))
        } else {
            None
        };
        let live = Arc::new(LiveBindings::from_config(&config));
        let mut hours = config.hours.clone();
        if let Ok(saved) = store.tenant_config(&config.tenant_id) {
            if let Ok(h) = serde_json::from_value::<WeeklyHours>(saved["hours"].clone()) {
                hours = h;
            }
        }
        #[cfg(feature = "vapi")]
        let vapi_adapter = if let Some(adapter) = voice_adapter {
            orchestrator
                .register(adapter.clone())
                .map_err(|e| ApiError::internal(format!("vapi adapter register: {e}")))?;
            Some(adapter)
        } else if live_providers && config.vapi_configured() {
            let adapter = rvoip_vapi::VapiApiKey::new(config.vapi_api_key.clone())
                .and_then(|key| rvoip_vapi::VapiAdapter::new(rvoip_vapi::VapiConfig::new(key)))
                .map_err(|e| ApiError::internal(format!("vapi voice adapter: {e}")))?;
            orchestrator
                .register(adapter.clone())
                .map_err(|e| ApiError::internal(format!("vapi adapter register: {e}")))?;
            Some(adapter)
        } else {
            None
        };
        let app = Self {
            state: AppState {
                conference_handoff: Arc::new(tokio::sync::Mutex::new(())),
                #[cfg(feature = "uctp")]
                uctp_adapter: Arc::new(std::sync::OnceLock::new()),
                #[cfg(feature = "media-webrtc")]
                conference_browser,
                config: Arc::new(config),
                store: Arc::new(store),
                orchestrator,
                sip_bound_port: Arc::new(Mutex::new(None)),
                uctp_bound: Arc::new(Mutex::new(None)),
                vapi,
                telnyx,
                telnyx_verifier,
                live,
                live_events: crate::events::channel(),
                hours: Arc::new(Mutex::new(hours)),
                #[cfg(feature = "vapi")]
                vapi_adapter,
                #[cfg(feature = "vapi")]
                vapi_calls: Arc::new(Mutex::new(HashMap::new())),
            },
        };
        spawn_event_mirror(app.state.clone());
        app.state.store.recover_conference_outbox()?;
        app.state.store.recover_conference_voice()?;
        crate::sms::outbox::spawn(app.state.clone());
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
        let uctp_addr = crate::uctp_host::start(self.state.clone()).await?;
        tracing::info!(%uctp_addr, "uctp host bound");
        crate::sip::bind(&self.state).await?;
        crate::conference_voice::bind(&self.state).await?;
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
        loop {
            match events.recv().await {
                Ok(event) => {
                    if let Err(err) = handle_event(&state, event).await {
                        tracing::debug!(error = %err, "event mirror skipped");
                    }
                }
                Err(broadcast::error::RecvError::Lagged(count)) => {
                    tracing::warn!(count, "event mirror lagged; continuing subscription");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

async fn handle_event(state: &AppState, event: Event) -> Result<()> {
    crate::conference_voice::mirror(state, &event)?;
    let tenant_id = state.config.tenant_id.as_str();
    match event {
        #[cfg(feature = "uctp")]
        Event::ConnectionPrincipalAuthenticated {
            connection_id,
            principal,
            ..
        } => {
            if let Err(error) =
                crate::uctp_host::admit_legacy(state, &connection_id, &principal).await
            {
                let _ = state
                    .orchestrator
                    .end_connection(connection_id, rvoip_core::adapter::EndReason::Normal)
                    .await;
                return Err(error);
            }
        }
        Event::DataMessageReceived {
            connection_id,
            message,
            ..
        } => {
            let body = String::from_utf8_lossy(&message.bytes).to_string();
            let sid = state
                .orchestrator
                .session_of(&connection_id)
                .ok_or_else(|| ApiError::not_found("data connection has no session"))?;
            let session = state
                .orchestrator
                .session(&sid)
                .ok_or_else(|| ApiError::not_found("data session missing"))?;
            let (cid, sender) = {
                let session = session
                    .read()
                    .map_err(|_| ApiError::internal("session lock"))?;
                let connection = session
                    .connections
                    .get(&connection_id)
                    .ok_or_else(|| ApiError::not_found("data connection missing"))?;
                (
                    session.conversation_id.to_string(),
                    connection.participant_id.to_string(),
                )
            };
            // Conference profile messages have explicit recipients and their own
            // durable dispatch. Legacy data frames must not bypass that contract.
            if !state.store.conference_members(tenant_id, &cid)?.is_empty() {
                return Err(ApiError::forbidden(
                    "conference messages require conversation-control/1",
                ));
            }
            let conv = state
                .store
                .get_conversation(tenant_id, &cid)?
                .ok_or_else(|| ApiError::not_found("data Conversation missing"))?;
            if conv.state != "open" {
                return Err(ApiError::conflict("Conversation closed"));
            }
            let provider_id = format!("uctp:{cid}:{sender}:{}", message.message_id);
            if state
                .store
                .find_message_by_provider(tenant_id, &provider_id)?
                .is_some()
            {
                return Ok(());
            }
            let row = MessageRow {
                id: format!("msg_{}", Uuid::new_v4().simple()),
                tenant_id: tenant_id.into(),
                conversation_id: cid.clone(),
                from_participant: Some(sender),
                medium: "chat".into(),
                body: body.clone(),
                provider_id: Some(provider_id),
                state: "accepted".into(),
                created_at: Utc::now().to_rfc3339(),
            };
            state.store.insert_message(&row)?;
            state.store.touch_conversation(tenant_id, &cid)?;
            crate::events::publish(state, tenant_id, Some(&cid), "message.posted");
            crate::vapi_chat::spawn_reply(state.clone(), row);
        }
        Event::ConnectionConnected { connection_id, .. } => {
            crate::vapi_voice::spawn_attach_on_connected(state.clone(), connection_id);
        }
        _ => {}
    }
    Ok(())
}
