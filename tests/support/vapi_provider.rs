//! Read-only provider verification for explicitly opted-in live rehearsals.
use serde_json::Value;
use std::{collections::BTreeSet, time::Duration};

pub async fn verify_ended_calls(
    key: &str,
    assistant: &str,
    cid: &str,
    sessions: &[String],
) -> Result<usize, &'static str> {
    let expected: BTreeSet<_> = sessions.iter().map(String::as_str).collect();
    if expected.is_empty() {
        return Ok(0);
    }
    let http = reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|_| "provider status client failed")?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(25);
    while tokio::time::Instant::now() < deadline {
        let response = http
            .get("https://api.vapi.ai/call")
            .query(&[("assistantId", assistant), ("limit", "25")])
            .bearer_auth(key)
            .send()
            .await
            .map_err(|_| "provider status request failed")?;
        if !response.status().is_success() {
            return Err("provider status request rejected");
        }
        let calls: Vec<Value> = response
            .json()
            .await
            .map_err(|_| "invalid provider status response")?;
        let matching: Vec<_> = calls
            .iter()
            .filter(|c| c["metadata"]["conversation_id"] == cid)
            .collect();
        let actual: BTreeSet<_> = matching
            .iter()
            .filter_map(|c| c["metadata"]["session_id"].as_str())
            .collect();
        if matching.len() > expected.len() || !actual.is_subset(&expected) {
            return Err("unexpected or duplicate provider calls for this Conversation");
        }
        if matching.len() == expected.len()
            && actual == expected
            && matching.iter().all(|c| {
                c["assistantId"] == assistant
                    && c["type"] == "vapi.websocketCall"
                    && c["status"] == "ended"
                    && c["endedAt"].as_str().is_some_and(|s| !s.is_empty())
            })
        {
            return Ok(matching.len());
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    Err("provider calls did not all reach a verified ended state")
}
