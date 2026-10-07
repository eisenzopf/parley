mod conference;
mod connections;
mod conversations;
mod events;
mod messages;
mod participants;
mod pickup;
mod sessions;
mod tenant;
mod webhooks;

use crate::auth::{self, AuthContext};
use crate::error::ApiError;
use crate::runtime::AppState;
use async_trait::async_trait;
use axum::extract::{FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::http::{header, HeaderValue};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, patch, post};
use axum::Router;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;

pub fn router(state: AppState) -> Router {
    Router::new()
        .nest_service(
            "/conference",
            ServeDir::new("web/conference").append_index_html_on_directories(true),
        )
        .nest_service("/uctp-client", ServeDir::new("clients/uctp-js"))
        .nest_service(
            "/widget",
            ServeDir::new("web/widget").append_index_html_on_directories(true),
        )
        .nest_service(
            "/desk",
            ServeDir::new("web/desk").append_index_html_on_directories(true),
        )
        .route("/healthz", get(healthz))
        .route("/v1/conference/:cid/tokens", post(conference::token))
        .route("/v1/public", get(tenant::public_config))
        .route("/v1/public/widget-token", post(tenant::public_widget_token))
        .route("/v1/events", get(events::stream))
        .route(
            "/v1/conversations",
            post(conversations::create).get(conversations::list),
        )
        .route("/v1/conversations/:cid", get(conversations::get_one))
        .route("/v1/conversations/:cid/close", post(conversations::close))
        .route(
            "/v1/conversations/:cid/timeline",
            get(conversations::timeline),
        )
        .route("/v1/conversations/:cid/vcon", get(conversations::vcon))
        .route(
            "/v1/conversations/:cid/sessions",
            post(sessions::create).get(sessions::list),
        )
        .route("/v1/sessions/:sid", get(sessions::get_one))
        .route("/v1/sessions/:sid/end", post(sessions::end))
        .route(
            "/v1/sessions/:sid/connections",
            get(connections::list_for_session),
        )
        .route("/v1/conversations/:cid/pickup", post(pickup::request))
        .route("/v1/conversations/:cid/pickup/accept", post(pickup::accept))
        .route(
            "/v1/conversations/:cid/return_to_ai",
            post(pickup::return_to_ai),
        )
        .route(
            "/v1/conversations/:cid/messages",
            post(messages::create).get(messages::list),
        )
        .route(
            "/v1/conversations/:cid/participants",
            get(participants::list).post(participants::add),
        )
        .route(
            "/v1/sessions/:sid/participants",
            post(participants::add_to_session),
        )
        .route(
            "/v1/participants/:pid/hand_off",
            post(participants::hand_off),
        )
        .route(
            "/v1/participants/:pid/take_over",
            post(participants::take_over),
        )
        .route("/v1/participants/:pid", patch(participants::patch))
        .route("/v1/participants/:pid/leave", post(participants::leave))
        .route("/v1/conversations/:cid/connections", get(connections::list))
        .route("/v1/connections/:id", get(connections::get_one))
        .route("/v1/tenant", get(tenant::get))
        .route(
            "/v1/tenant/hours",
            get(tenant::hours).put(tenant::put_hours),
        )
        .route("/v1/tenant/ai", get(tenant::ai))
        .route("/v1/tenant/pickup", get(tenant::pickup))
        .route("/v1/tenant/widget", get(tenant::widget))
        .route("/v1/tenant/numbers", get(tenant::numbers))
        .route("/v1/widget/tokens", post(tenant::widget_token))
        .route("/v1/operators/bootstrap", post(tenant::bootstrap))
        .route("/v1/webhooks", get(webhooks::list).post(webhooks::create))
        .route("/v1/vapi/tools", post(webhooks::vapi_tools))
        .route("/v1/test/sms/inbound", post(webhooks::sms_inbound_dev))
        .route("/v1/sms/inbound", post(webhooks::sms_inbound))
        .with_state(state)
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store, no-cache, must-revalidate"),
        ))
        .layer(middleware::from_fn(no_store))
}

async fn no_store(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    res.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, no-cache, must-revalidate"),
    );
    res
}

async fn healthz(State(state): State<crate::runtime::AppState>) -> axum::Json<serde_json::Value> {
    let sqlite_ok = state
        .store
        .list_conversations(&state.config.tenant_id)
        .is_ok();
    let blob = std::path::Path::new(&state.config.blob_dir);
    let blob_ok = std::fs::create_dir_all(blob).is_ok() && blob.is_dir();
    let uctp = state
        .uctp_bound
        .lock()
        .ok()
        .and_then(|g| *g)
        .map(|a| a.to_string());
    let sip = state.sip_bound_port.lock().ok().and_then(|g| *g);
    axum::Json(serde_json::json!({
        "ok": sqlite_ok && blob_ok,
        "sqlite": sqlite_ok,
        "blob_dir": state.config.blob_dir,
        "blob_writable": blob_ok,
        "uctp": uctp,
        "sip": sip,
        "vapi": state.vapi.is_some(),
        "telnyx": state.telnyx.is_some(),
        "assistant_id_set": !state.config.vapi_assistant_id.is_empty(),
        "telnyx_from_set": !state.config.telnyx_from.is_empty(),
        "chat_mode": state.config.vapi_chat_mode,
        "hostname": state.config.public_hostname,
        "counters": crate::observe::snapshot(),
    }))
}

pub struct Auth(pub AuthContext);

#[async_trait]
impl FromRequestParts<AppState> for Auth {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok());
        let ctx = auth::authenticate(&state.store, &state.config, header)?;
        Ok(Auth(ctx))
    }
}

pub fn require_server(auth: &AuthContext) -> Result<(), ApiError> {
    match auth.actor {
        crate::auth::Actor::ApiSecret | crate::auth::Actor::Operator { .. } => Ok(()),
        crate::auth::Actor::Widget { .. } => {
            Err(ApiError::forbidden("widget token cannot manage tenant"))
        }
    }
}
