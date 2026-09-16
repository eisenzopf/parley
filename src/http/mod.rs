mod connections;
mod conversations;
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
use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::routing::{get, patch, post};
use axum::Router;
use tower_http::services::ServeDir;

pub fn router(state: AppState) -> Router {
    Router::new()
        .nest_service("/widget", ServeDir::new("web/widget"))
        .nest_service("/desk", ServeDir::new("web/desk"))
        .route("/healthz", get(healthz))
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
        .route(
            "/v1/conversations/:cid/pickup",
            post(pickup::request),
        )
        .route(
            "/v1/conversations/:cid/pickup/accept",
            post(pickup::accept),
        )
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
        .route(
            "/v1/conversations/:cid/connections",
            get(connections::list),
        )
        .route("/v1/connections/:id", get(connections::get_one))
        .route("/v1/tenant", get(tenant::get))
        .route("/v1/widget/tokens", post(tenant::widget_token))
        .route("/v1/operators/bootstrap", post(tenant::bootstrap))
        .route("/v1/webhooks", get(webhooks::list).post(webhooks::create))
        .route("/v1/vapi/tools", post(webhooks::vapi_tools))
        .route("/v1/test/sms/inbound", post(webhooks::sms_inbound_dev))
        .route("/v1/sms/inbound", post(webhooks::sms_inbound))
        .with_state(state)
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
