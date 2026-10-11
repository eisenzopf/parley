//! Live SMS admission uses reviewed web enrollment, never roster membership.
//! Telnyx remains authoritative for STOP and profile-wide opt-out enforcement.
use crate::{
    config::Config,
    error::{ApiError, Result},
};
use serde::Deserialize;

pub const SOURCE: &str = "https://rudeless.ai/sms";
pub const BRAND: &str = "Rudeless Thelve: ";
pub const OPT_OUT: &str = "Reply STOP to opt out.";

#[derive(Deserialize)]
struct Enrollments {
    version: u32,
    source: String,
    sender_number: String,
    campaign_id: String,
    recipients: Vec<Recipient>,
}

#[derive(Deserialize)]
struct Recipient {
    number: String,
    web_enrollment_confirmed: bool,
    reviewed_at: String,
    evidence: String,
    #[serde(default)]
    revoked: bool,
}

/// Re-read at admission and submission so a withdrawn approval blocks queued work.
pub fn authorize(config: &Config, from: &str, to: &str, body: &str) -> Result<()> {
    if config.vapi_chat_mode == "fake" {
        return Ok(());
    }
    let deny = || {
        ApiError::forbidden("Live SMS requires reviewed enrollment at rudeless.ai/sms for this recipient and sender")
    };
    let bytes = std::fs::read(&config.sms_enrollment_path).map_err(|_| deny())?;
    let records: Enrollments = serde_json::from_slice(&bytes).map_err(|_| deny())?;
    if records.version != 1
        || records.source != SOURCE
        || records.sender_number != from
        || from != config.telnyx_from
        || records.campaign_id.trim().is_empty()
        || records.campaign_id != config.sms_campaign_id
    {
        return Err(deny());
    }
    let matches: Vec<_> = records
        .recipients
        .iter()
        .filter(|r| r.number == to)
        .collect();
    if matches.len() != 1
        || !matches[0].web_enrollment_confirmed
        || matches[0].revoked
        || chrono::DateTime::parse_from_rfc3339(&matches[0].reviewed_at).is_err()
        || matches[0].evidence.trim().is_empty()
    {
        return Err(deny());
    }
    if !body.starts_with(BRAND) || !body.ends_with(OPT_OUT) {
        return Err(ApiError::bad_request(
            "SMS must use the approved Rudeless Thelve branding and STOP disclosure",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn live_admission_requires_review_and_campaign_format_and_rechecks_withdrawal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("enrollments.json");
        let mut cfg = Config::default();
        cfg.vapi_chat_mode = "live".into();
        cfg.telnyx_from = "+14155550000".into();
        cfg.sms_enrollment_path = path.display().to_string();
        cfg.sms_campaign_id = "approved-test-campaign".into();
        let body = "Rudeless Thelve: Your requested demo is confirmed. Reply STOP to opt out.";
        assert!(authorize(&cfg, &cfg.telnyx_from, "+14155550101", body).is_err());
        let mut records = json!({"version":1,"source":SOURCE,"sender_number":cfg.telnyx_from,
            "campaign_id":"approved-test-campaign","recipients":[{"number":"+14155550101",
            "web_enrollment_confirmed":true,"reviewed_at":"2026-10-10T19:00:00Z",
            "evidence":"Recipient submitted the public form; operator reviewed the requested demo"}]});
        std::fs::write(&path, records.to_string()).unwrap();
        assert!(authorize(&cfg, &cfg.telnyx_from, "+14155550101", body).is_ok());
        assert!(authorize(&cfg, &cfg.telnyx_from, "+14155550102", body).is_err());
        assert!(authorize(&cfg, "+14155550001", "+14155550101", body).is_err());
        assert!(authorize(&cfg, &cfg.telnyx_from, "+14155550101", "Unbranded update").is_err());
        records["recipients"][0]["revoked"] = json!(true);
        std::fs::write(&path, records.to_string()).unwrap();
        assert!(authorize(&cfg, &cfg.telnyx_from, "+14155550101", body).is_err());
        std::fs::write(&path, "malformed").unwrap();
        assert!(authorize(&cfg, &cfg.telnyx_from, "+14155550101", body).is_err());
    }
}
