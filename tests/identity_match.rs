use chrono::{Duration, Utc};
use parley::config::Config;
use parley::identity::{resolve_ingress, IngressKeys, Match};
use parley::store::{ConversationRow, Store};
use serde_json::json;

fn store() -> (Store, Config, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut cfg = Config::default();
    cfg.sqlite_path = dir.path().join("parley.sqlite").display().to_string();
    cfg.blob_dir = dir.path().join("blobs").display().to_string();
    let store = Store::open(&cfg).expect("store");
    (store, cfg, dir)
}

fn insert_open(store: &Store, tenant: &str, cid: &str) {
    let now = Utc::now().to_rfc3339();
    store
        .insert_conversation(&ConversationRow {
            id: cid.into(),
            tenant_id: tenant.into(),
            state: "open".into(),
            policy: "persistent".into(),
            opened_at: now.clone(),
            closed_at: None,
            last_activity_at: now,
            vapi_chat_session_id: None,
            metadata: json!({}),
        })
        .unwrap();
}

fn insert_closed(store: &Store, tenant: &str, cid: &str, closed_at: chrono::DateTime<Utc>) {
    let now = closed_at.to_rfc3339();
    store
        .insert_conversation(&ConversationRow {
            id: cid.into(),
            tenant_id: tenant.into(),
            state: "closed".into(),
            policy: "persistent".into(),
            opened_at: now.clone(),
            closed_at: Some(now.clone()),
            last_activity_at: now,
            vapi_chat_session_id: None,
            metadata: json!({}),
        })
        .unwrap();
}

#[test]
fn e164_continues_open_conversation() {
    let (store, cfg, _dir) = store();
    insert_open(&store, &cfg.tenant_id, "conv_open");
    store
        .upsert_identity(&cfg.tenant_id, "e164", "+14155550111", "conv_open", false)
        .unwrap();
    let m = resolve_ingress(
        &store,
        &cfg.tenant_id,
        &IngressKeys {
            e164: Some("+14155550111".into()),
            ..Default::default()
        },
        cfg.reopen_window_secs,
        Utc::now(),
    )
    .unwrap();
    assert_eq!(m, Match::Continue("conv_open".into()));
}

#[test]
fn visitor_id_is_preferred_over_e164() {
    let (store, cfg, _dir) = store();
    insert_open(&store, &cfg.tenant_id, "conv_visitor");
    insert_open(&store, &cfg.tenant_id, "conv_e164");
    store
        .upsert_identity(&cfg.tenant_id, "visitor_id", "usr_123", "conv_visitor", false)
        .unwrap();
    store
        .upsert_identity(&cfg.tenant_id, "e164", "+14155550111", "conv_e164", false)
        .unwrap();
    let m = resolve_ingress(
        &store,
        &cfg.tenant_id,
        &IngressKeys {
            e164: Some("+14155550111".into()),
            visitor_id: Some("usr_123".into()),
            cookie: None,
        },
        cfg.reopen_window_secs,
        Utc::now(),
    )
    .unwrap();
    assert_eq!(m, Match::Continue("conv_visitor".into()));
}

#[test]
fn closed_within_window_reopens() {
    let (store, cfg, _dir) = store();
    let closed = Utc::now() - Duration::hours(1);
    insert_closed(&store, &cfg.tenant_id, "conv_closed", closed);
    store
        .upsert_identity(&cfg.tenant_id, "e164", "+14155550111", "conv_closed", false)
        .unwrap();
    let m = resolve_ingress(
        &store,
        &cfg.tenant_id,
        &IngressKeys {
            e164: Some("+14155550111".into()),
            ..Default::default()
        },
        cfg.reopen_window_secs,
        Utc::now(),
    )
    .unwrap();
    assert_eq!(m, Match::Reopen("conv_closed".into()));
}

#[test]
fn closed_outside_window_opens_new() {
    let (store, cfg, _dir) = store();
    let closed = Utc::now() - Duration::days(30);
    insert_closed(&store, &cfg.tenant_id, "conv_old", closed);
    store
        .upsert_identity(&cfg.tenant_id, "e164", "+14155550111", "conv_old", false)
        .unwrap();
    let m = resolve_ingress(
        &store,
        &cfg.tenant_id,
        &IngressKeys {
            e164: Some("+14155550111".into()),
            ..Default::default()
        },
        cfg.reopen_window_secs,
        Utc::now(),
    )
    .unwrap();
    assert_eq!(m, Match::OpenNew);
}

#[test]
fn do_not_reopen_opens_new() {
    let (store, cfg, _dir) = store();
    insert_closed(&store, &cfg.tenant_id, "conv_dnr", Utc::now());
    store
        .upsert_identity(&cfg.tenant_id, "cookie", "ck_1", "conv_dnr", true)
        .unwrap();
    let m = resolve_ingress(
        &store,
        &cfg.tenant_id,
        &IngressKeys {
            cookie: Some("ck_1".into()),
            ..Default::default()
        },
        cfg.reopen_window_secs,
        Utc::now(),
    )
    .unwrap();
    assert_eq!(m, Match::OpenNew);
}

#[test]
fn unknown_keys_open_new() {
    let (store, cfg, _dir) = store();
    let m = resolve_ingress(
        &store,
        &cfg.tenant_id,
        &IngressKeys {
            e164: Some("+14155550999".into()),
            ..Default::default()
        },
        cfg.reopen_window_secs,
        Utc::now(),
    )
    .unwrap();
    assert_eq!(m, Match::OpenNew);
}
