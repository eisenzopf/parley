use std::sync::atomic::{AtomicU64, Ordering};

pub static CONVERSATIONS_OPENED: AtomicU64 = AtomicU64::new(0);
pub static PICKUPS: AtomicU64 = AtomicU64::new(0);
pub static HANDOFFS: AtomicU64 = AtomicU64::new(0);
pub static SMS_FAIL: AtomicU64 = AtomicU64::new(0);

pub fn conversation_opened() {
    CONVERSATIONS_OPENED.fetch_add(1, Ordering::Relaxed);
}

pub fn pickup() {
    PICKUPS.fetch_add(1, Ordering::Relaxed);
}

pub fn snapshot() -> serde_json::Value {
    serde_json::json!({
        "parley_conversations_opened": CONVERSATIONS_OPENED.load(Ordering::Relaxed),
        "parley_pickups": PICKUPS.load(Ordering::Relaxed),
        "parley_handoffs": HANDOFFS.load(Ordering::Relaxed),
        "parley_sms_fail": SMS_FAIL.load(Ordering::Relaxed),
    })
}
