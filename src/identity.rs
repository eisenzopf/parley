use crate::error::{ApiError, Result};
use crate::store::{ConversationRow, Store};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct IngressKeys {
    pub e164: Option<String>,
    pub visitor_id: Option<String>,
    pub cookie: Option<String>,
}

impl IngressKeys {
    pub fn is_empty(&self) -> bool {
        self.e164.is_none() && self.visitor_id.is_none() && self.cookie.is_none()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&'static str, &str)> {
        [
            ("visitor_id", self.visitor_id.as_deref()),
            ("e164", self.e164.as_deref()),
            ("cookie", self.cookie.as_deref()),
        ]
        .into_iter()
        .filter_map(|(k, v)| v.map(|v| (k, v)))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Match {
    Continue(String),
    Reopen(String),
    OpenNew,
}

pub fn resolve_ingress(
    store: &Store,
    tenant_id: &str,
    keys: &IngressKeys,
    reopen_window_secs: i64,
    now: DateTime<Utc>,
) -> Result<Match> {
    if keys.is_empty() {
        return Err(ApiError::bad_request(
            "identity requires e164, visitor_id, or cookie",
        ));
    }
    for (key_type, key_value) in keys.iter() {
        if let Some(hit) = store.lookup_identity(tenant_id, key_type, key_value)? {
            return classify_hit(store, tenant_id, &hit.conversation_id, hit.do_not_reopen, reopen_window_secs, now);
        }
    }
    Ok(Match::OpenNew)
}

fn classify_hit(
    store: &Store,
    tenant_id: &str,
    cid: &str,
    do_not_reopen: bool,
    reopen_window_secs: i64,
    now: DateTime<Utc>,
) -> Result<Match> {
    let Some(conv) = store.get_conversation(tenant_id, cid)? else {
        return Ok(Match::OpenNew);
    };
    match conv.state.as_str() {
        "open" => Ok(Match::Continue(cid.to_string())),
        "closed" => {
            if do_not_reopen {
                return Ok(Match::OpenNew);
            }
            if can_reopen(&conv, reopen_window_secs, now) {
                Ok(Match::Reopen(cid.to_string()))
            } else {
                Ok(Match::OpenNew)
            }
        }
        _ => Ok(Match::OpenNew),
    }
}

fn can_reopen(conv: &ConversationRow, reopen_window_secs: i64, now: DateTime<Utc>) -> bool {
    let Some(closed) = conv.closed_at.as_deref() else {
        return false;
    };
    let Ok(closed_at) = DateTime::parse_from_rfc3339(closed) else {
        return false;
    };
    let closed_at = closed_at.with_timezone(&Utc);
    now < closed_at + Duration::seconds(reopen_window_secs)
}
