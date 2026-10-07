//! Live Vapi assistant + Telnyx SMS profile. Creates each resource **once**,
//! then reuses IDs from `var/provision.json` and the provider APIs.

use crate::config::Config;
use crate::error::ApiError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use telnyx::messaging::{
    CreateMessagingProfileParams, ListMessagingProfilesParams, UpdateMessagingProfileParams,
};
use telnyx::numbers::{ListPhoneNumbersParams, UpdateMessagingSettingsParams};
use vapi::VapiClient;

const ASSISTANT_NAME: &str = "Parley";
const MESSAGING_PROFILE_NAME: &str = "Parley";
const LAB_SMS: &str = "+18058253932";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SavedProvision {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    assistant_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    messaging_profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    telnyx_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    phone_number_id: Option<String>,
}

pub fn provision_path(config: &Config) -> std::path::PathBuf {
    config.state_dir().join("provision.json")
}

pub async fn ensure(config: &mut Config) -> Result<(), ApiError> {
    crate::tls::ensure_rustls_ring();
    let path = provision_path(config);
    let mut saved = load(&path).unwrap_or_default();
    apply_saved(config, &saved);

    if !config.provision {
        return Ok(());
    }

    let mut changed = false;
    if config.vapi_configured() {
        changed |= resolve_assistant(config, &mut saved).await?;
    }
    if config.telnyx_configured() {
        changed |= resolve_sms(config, &mut saved).await?;
    }
    if changed {
        save(&path, &saved)?;
    }
    Ok(())
}

fn apply_saved(config: &mut Config, saved: &SavedProvision) {
    if config.vapi_assistant_id.is_empty() {
        if let Some(id) = &saved.assistant_id {
            config.vapi_assistant_id = id.clone();
        }
    }
    if config.telnyx_messaging_profile_id.is_empty() {
        if let Some(id) = &saved.messaging_profile_id {
            config.telnyx_messaging_profile_id = id.clone();
        }
    }
    if config.telnyx_from.is_empty() {
        if let Some(from) = &saved.telnyx_from {
            config.telnyx_from = from.clone();
        }
    }
}

async fn resolve_assistant(
    config: &mut Config,
    saved: &mut SavedProvision,
) -> Result<bool, ApiError> {
    let client = VapiClient::new(config.vapi_api_key.clone())
        .map_err(|e| ApiError::internal(format!("vapi client: {e}")))?;
    let server_url = format!(
        "{}/v1/vapi/tools",
        config.public_http_base().trim_end_matches('/')
    );

    let mut ids = Vec::new();
    if !config.vapi_assistant_id.is_empty() {
        ids.push(config.vapi_assistant_id.clone());
    }
    if let Some(id) = &saved.assistant_id {
        if !ids.contains(id) {
            ids.push(id.clone());
        }
    }

    for id in ids {
        match client.assistants().get(&id).await {
            Ok(existing) => {
                let current = assistant_server_url(&existing);
                sync_assistant_server_url(&client, &existing.id, current.as_deref(), &server_url)
                    .await;
                let dirty = saved.assistant_id.as_deref() != Some(existing.id.as_str());
                remember_assistant(config, saved, existing.id);
                tracing::info!(assistant_id = %config.vapi_assistant_id, "reusing Parley Vapi assistant");
                return Ok(dirty);
            }
            Err(e) if e.status_code() == Some(404) => continue,
            Err(e) => return Err(ApiError::internal(format!("vapi get assistant: {e}"))),
        }
    }

    let assistants = client
        .assistants()
        .list()
        .await
        .map_err(|e| ApiError::internal(format!("vapi list assistants: {e}")))?;
    if let Some(existing) = assistants
        .into_iter()
        .find(|a| a.display_name() == ASSISTANT_NAME)
    {
        sync_assistant_server_url(
            &client,
            &existing.id,
            assistant_server_url(&existing).as_deref(),
            &server_url,
        )
        .await;
        let created_new = saved.assistant_id.as_deref() != Some(existing.id.as_str());
        remember_assistant(config, saved, existing.id);
        tracing::info!(assistant_id = %config.vapi_assistant_id, "reusing named Parley Vapi assistant");
        return Ok(created_new);
    }

    let mut body: Value = serde_json::from_str(include_str!("../config/demo-assistant.json"))
        .map_err(|e| ApiError::internal(format!("demo assistant json: {e}")))?;
    body["name"] = json!(ASSISTANT_NAME);
    apply_server_url(&mut body, &server_url);
    let created = client
        .assistants()
        .create_json(&body)
        .await
        .map_err(|e| ApiError::internal(format!("vapi create assistant: {e}")))?;
    remember_assistant(config, saved, created.id);
    tracing::info!(assistant_id = %config.vapi_assistant_id, "created Parley Vapi assistant");
    Ok(true)
}

async fn sync_assistant_server_url(
    client: &VapiClient,
    id: &str,
    current: Option<&str>,
    server_url: &str,
) {
    if current == Some(server_url) {
        return;
    }
    if let Err(e) = client
        .assistants()
        .update(id, &json!({ "server": { "url": server_url } }))
        .await
    {
        tracing::warn!(error = %e, "vapi assistant server url update skipped");
    }
}

fn assistant_server_url(assistant: &vapi::Assistant) -> Option<String> {
    if let Some(url) = assistant.server_url.as_deref().filter(|u| !u.is_empty()) {
        return Some(url.to_string());
    }
    assistant
        .extra
        .get("server")
        .and_then(|s| s.get("url"))
        .and_then(|u| u.as_str())
        .filter(|u| !u.is_empty())
        .map(str::to_string)
}

fn apply_server_url(body: &mut Value, server_url: &str) {
    body["server"] = json!({ "url": server_url });
    if let Some(obj) = body.as_object_mut() {
        obj.remove("serverUrl");
    }
    if let Some(tools) = body
        .pointer_mut("/model/tools")
        .and_then(|v| v.as_array_mut())
    {
        for tool in tools {
            tool["server"] = json!({ "url": server_url });
        }
    }
}

fn remember_assistant(config: &mut Config, saved: &mut SavedProvision, id: String) {
    config.vapi_assistant_id = id.clone();
    saved.assistant_id = Some(id);
}

async fn resolve_sms(config: &mut Config, saved: &mut SavedProvision) -> Result<bool, ApiError> {
    if config.telnyx_from.is_empty() {
        config.telnyx_from = saved
            .telnyx_from
            .clone()
            .unwrap_or_else(|| LAB_SMS.to_string());
    }
    let client = telnyx::Client::builder()
        .api_key(config.telnyx_api_key.clone())
        .build()
        .map_err(|e| ApiError::internal(format!("telnyx client: {e}")))?;
    let webhook = format!(
        "{}/v1/sms/inbound",
        config.public_http_base().trim_end_matches('/')
    );

    let mut created = false;
    let profile_id = if let Some(id) = existing_profile_id(config, saved) {
        match client.messaging_profiles().get(&id).await {
            Ok(profile) => {
                sync_profile_webhook(&client, &id, profile.webhook_url.as_deref(), &webhook).await;
                id
            }
            Err(e) if e.is_not_found() => {
                resolve_or_create_profile(&client, &webhook, &mut created).await?
            }
            Err(e) => return Err(ApiError::internal(format!("telnyx get profile: {e}"))),
        }
    } else {
        resolve_or_create_profile(&client, &webhook, &mut created).await?
    };
    let mut dirty = created
        || saved.messaging_profile_id.as_deref() != Some(profile_id.as_str())
        || saved.telnyx_from.as_deref() != Some(config.telnyx_from.as_str());
    config.telnyx_messaging_profile_id = profile_id.clone();
    saved.messaging_profile_id = Some(profile_id.clone());
    saved.telnyx_from = Some(config.telnyx_from.clone());

    let numbers = client
        .phone_numbers()
        .list(ListPhoneNumbersParams {
            filter_phone_number: Some(config.telnyx_from.clone()),
            page_size: Some(5),
            ..Default::default()
        })
        .await
        .map_err(|e| ApiError::internal(format!("telnyx list numbers: {e}")))?;
    let Some(number) = numbers.data.into_iter().next() else {
        tracing::warn!(from = %config.telnyx_from, "telnyx lab number not on this account");
        return Ok(dirty);
    };
    if let Some(id) = number.id.clone() {
        if saved.phone_number_id.as_deref() != Some(id.as_str()) {
            dirty = true;
        }
        saved.phone_number_id = Some(id.clone());
        if number.messaging_profile_id.as_deref() != Some(profile_id.as_str()) {
            client
                .phone_numbers()
                .update_messaging_settings(
                    &id,
                    UpdateMessagingSettingsParams {
                        messaging_profile_id: Some(profile_id.clone()),
                        ..Default::default()
                    },
                )
                .await
                .map_err(|e| ApiError::internal(format!("telnyx attach number: {e}")))?;
        }
    }
    tracing::info!(from = %config.telnyx_from, profile = %profile_id, "reusing Telnyx SMS number");
    Ok(dirty)
}

fn existing_profile_id(config: &Config, saved: &SavedProvision) -> Option<String> {
    if !config.telnyx_messaging_profile_id.is_empty() {
        Some(config.telnyx_messaging_profile_id.clone())
    } else {
        saved.messaging_profile_id.clone()
    }
}

async fn resolve_or_create_profile(
    client: &telnyx::Client,
    webhook: &str,
    created: &mut bool,
) -> Result<String, ApiError> {
    let listed = client
        .messaging_profiles()
        .list(ListMessagingProfilesParams {
            filter_name_eq: Some(MESSAGING_PROFILE_NAME.into()),
            page_size: Some(20),
            ..Default::default()
        })
        .await
        .map_err(|e| ApiError::internal(format!("telnyx list profiles: {e}")))?;
    if let Some(id) = listed.data.iter().find_map(|p| p.id.clone()) {
        let current = listed
            .data
            .iter()
            .find(|p| p.id.as_deref() == Some(id.as_str()))
            .and_then(|p| p.webhook_url.clone());
        sync_profile_webhook(client, &id, current.as_deref(), webhook).await;
        return Ok(id);
    }
    let mut params = CreateMessagingProfileParams::new(MESSAGING_PROFILE_NAME, vec!["US".into()]);
    params.webhook_url = Some(webhook.to_string());
    let id = client
        .messaging_profiles()
        .create(params)
        .await
        .map_err(|e| ApiError::internal(format!("telnyx create profile: {e}")))?
        .id
        .ok_or_else(|| ApiError::internal("telnyx profile missing id"))?;
    *created = true;
    tracing::info!(profile_id = %id, "created Telnyx messaging profile");
    Ok(id)
}

async fn sync_profile_webhook(
    client: &telnyx::Client,
    id: &str,
    current: Option<&str>,
    webhook: &str,
) {
    if current == Some(webhook) {
        return;
    }
    let mut params = UpdateMessagingProfileParams::default();
    params.webhook_url = Some(webhook.to_string());
    if let Err(e) = client.messaging_profiles().update(id, params).await {
        tracing::warn!(error = %e, "telnyx profile webhook update skipped");
    }
}

fn load(path: &std::path::Path) -> Option<SavedProvision> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn save(path: &std::path::Path, saved: &SavedProvision) -> Result<(), ApiError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| ApiError::internal(format!("var dir: {e}")))?;
    }
    let json = serde_json::to_string_pretty(saved)
        .map_err(|e| ApiError::internal(format!("encode provision: {e}")))?;
    std::fs::write(path, json).map_err(|e| ApiError::internal(format!("write provision: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn saved_file_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("provision.json");
        let saved = SavedProvision {
            assistant_id: Some("asst_parley".into()),
            messaging_profile_id: Some("prof_1".into()),
            telnyx_from: Some(LAB_SMS.into()),
            phone_number_id: Some("num_1".into()),
        };
        save(&path, &saved).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.assistant_id.as_deref(), Some("asst_parley"));
        assert_eq!(loaded.messaging_profile_id.as_deref(), Some("prof_1"));
        let mut cfg = Config::default();
        apply_saved(&mut cfg, &loaded);
        assert_eq!(cfg.vapi_assistant_id, "asst_parley");
        assert_eq!(cfg.telnyx_from, LAB_SMS);
        assert_eq!(cfg.telnyx_messaging_profile_id, "prof_1");
    }

    #[tokio::test]
    async fn provision_disabled_does_not_create() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.blob_dir = dir.path().join("blobs").display().to_string();
        cfg.provision = false;
        cfg.vapi_api_key = "sk-test".into();
        cfg.telnyx_api_key = "KEY0123".into();
        ensure(&mut cfg).await.unwrap();
        assert!(cfg.vapi_assistant_id.is_empty());
        assert!(cfg.telnyx_messaging_profile_id.is_empty());
        assert!(!provision_path(&cfg).exists());
    }
}
