use crate::auth::{self, AuthContext};
use crate::error::ApiError;
use crate::events;
use crate::runtime::AppState;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::stream::{self, Stream};
use serde::Deserialize;
use std::convert::Infallible;
use std::time::Duration;

#[derive(Deserialize)]
pub struct EventsQuery {
    pub access_token: Option<String>,
}

pub async fn stream(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<EventsQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let auth = authenticate_stream(&state, &headers, query.access_token.as_deref())?;
    let rx = state.live_events.subscribe();
    let tenant_id = auth.tenant_id.clone();
    let actor = auth.actor.clone();
    let store = state.store.clone();
    let stream = stream::unfold(rx, move |mut rx| {
        let tenant_id = tenant_id.clone();
        let actor = actor.clone();
        let store = store.clone();
        async move {
            loop {
                match rx.recv().await {
                    Ok(ev) => {
                        if !events::visible_to(&store, &tenant_id, &actor, &ev) {
                            continue;
                        }
                        match Event::default().event(ev.verb.clone()).json_data(&ev) {
                            Ok(sse) => return Some((Ok(sse), rx)),
                            Err(_) => continue,
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        let sse = Event::default().event("resync").data("{}");
                        return Some((Ok(sse), rx));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
                }
            }
        }
    });
    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ka"),
    ))
}

fn authenticate_stream(
    state: &AppState,
    headers: &HeaderMap,
    access_token: Option<&str>,
) -> Result<AuthContext, ApiError> {
    let header = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());
    if header.is_some() {
        return auth::authenticate(&state.store, &state.config, header);
    }
    auth::authenticate(&state.store, &state.config, access_token)
}
