use crate::error::Result;
use crate::runtime::AppState;
use serde_json::json;
use std::fs;
use std::path::PathBuf;

pub fn wrap_conversation(state: &AppState, tenant_id: &str, cid: &str) -> Result<String> {
    let dir = PathBuf::from(&state.config.blob_dir).join("vcon");
    fs::create_dir_all(&dir).ok();
    let path = dir.join(format!("{cid}.json"));
    let messages = state.store.list_messages(tenant_id, cid)?;
    let sessions = state.store.list_sessions(tenant_id, cid)?;
    let message_ids: Vec<String> = messages.iter().map(|m| m.id.clone()).collect();
    let session_ids: Vec<String> = sessions.iter().map(|s| s.id.clone()).collect();
    let body = json!({
        "vcon": "0.0.2",
        "uuid": cid,
        "parties": state.store.list_participants(tenant_id, cid)?,
        "dialog": sessions,
        "analysis": messages,
        "parley.vcon.conversation.v1": {
            "cid": cid,
            "session_vcons": session_ids,
            "message_ids": message_ids,
        }
    });
    fs::write(&path, serde_json::to_vec_pretty(&body).unwrap_or_default()).ok();
    let stored = path.display().to_string();
    state
        .store
        .insert_vcon(tenant_id, Some(cid), None, &stored)?;
    Ok(stored)
}
