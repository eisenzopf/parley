use crate::conversation::{self, CreateConversation};
use crate::error::ApiError;
use crate::http::Auth;
use crate::runtime::AppState;
use axum::extract::{Path, State};
use axum::Json;
use serde_json::{json, Value};

pub async fn create(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Json(body): Json<CreateConversation>,
) -> Result<Json<Value>, ApiError> {
    let view = conversation::create_or_continue(&state, &auth.tenant_id, body).await?;
    Ok(Json(serde_json::to_value(view).unwrap_or(json!({}))))
}

pub async fn list(
    State(state): State<AppState>,
    Auth(auth): Auth,
) -> Result<Json<Value>, ApiError> {
    let rows = state.store.list_conversations(&auth.tenant_id)?;
    let mut conversations = Vec::new();
    for row in rows {
        let sessions = state.store.list_sessions(&auth.tenant_id, &row.id)?;
        let live_voice = sessions
            .iter()
            .any(|s| s.medium == "voice" && s.ended_at.is_none());
        let events = state.store.list_events(&auth.tenant_id, &row.id)?;
        let requested = events.iter().any(|e| e.event_type == "pickup.requested");
        let accepted = events.iter().any(|e| e.event_type == "pickup.accepted");
        let waiting_pickup = requested && !accepted;
        let messages = state.store.list_messages(&auth.tenant_id, &row.id)?;
        let unread = messages
            .iter()
            .any(|m| m.medium == "sms" && m.state == "accepted");
        let preview = messages.last().map(|m| m.body.clone()).unwrap_or_default();
        let identities = state
            .store
            .identities_for_conversation(&auth.tenant_id, &row.id)?;
        let title = identities
            .iter()
            .find(|i| i.key_type == "e164")
            .or_else(|| identities.iter().find(|i| i.key_type == "visitor_id"))
            .map(|i| i.key_value.clone())
            .unwrap_or_else(|| "Someone".into());
        let preview: String = preview.chars().take(160).collect();
        let mut item = serde_json::to_value(&row).unwrap_or(json!({}));
        if let Some(obj) = item.as_object_mut() {
            obj.insert("live_voice".into(), json!(live_voice));
            obj.insert("waiting_pickup".into(), json!(waiting_pickup));
            obj.insert("unread".into(), json!(unread));
            obj.insert("title".into(), json!(title));
            obj.insert("preview".into(), json!(preview));
        }
        conversations.push(item);
    }
    Ok(Json(json!({ "conversations": conversations })))
}

pub async fn get_one(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(cid): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let view = conversation::view(&state, &auth.tenant_id, &cid, "get")?;
    Ok(Json(serde_json::to_value(view).unwrap_or(json!({}))))
}

pub async fn close(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(cid): Path<String>,
) -> Result<Json<Value>, ApiError> {
    if !state
        .store
        .conference_members(&auth.tenant_id, &cid)?
        .is_empty()
        && !matches!(auth.actor, crate::auth::Actor::ApiSecret)
    {
        return Err(ApiError::forbidden(
            "conference HTTP close requires administrator; owners use UCTP",
        ));
    }
    let view = conversation::close(&state, &auth.tenant_id, &cid).await?;
    Ok(Json(serde_json::to_value(view).unwrap_or(json!({}))))
}

pub async fn timeline(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(cid): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let tl = conversation::timeline(&state, &auth.tenant_id, &cid)?;
    Ok(Json(serde_json::to_value(tl).unwrap_or(json!({}))))
}

pub async fn vcon(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(cid): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let path = state.store.get_vcon_path(&auth.tenant_id, &cid)?;
    match path {
        Some(p) => {
            let body = std::fs::read_to_string(&p).unwrap_or_else(|_| "{}".into());
            let value: Value = serde_json::from_str(&body).unwrap_or(json!({ "path": p }));
            Ok(Json(value))
        }
        None => Err(ApiError::not_found("vcon not wrapped yet")),
    }
}
