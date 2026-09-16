use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use parley::config::Config;
use parley::store::Store;
use parley::App;
use serde_json::{json, Value};
use tower::ServiceExt;

fn test_config(dir: &std::path::Path) -> Config {
    let mut cfg = Config::default();
    cfg.sqlite_path = dir.join("parley.sqlite").display().to_string();
    cfg.blob_dir = dir.join("blobs").display().to_string();
    cfg.api_secret = "dev-only".into();
    cfg
}

async fn json(
    router: axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", "Bearer dev-only")
        .header("content-type", "application/json");
    let req = if let Some(body) = body {
        builder.body(Body::from(serde_json::to_vec(&body).unwrap())).unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
    };
    let response = router.oneshot(req).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

#[tokio::test]
async fn fake_inbound_sms_continues_conversation() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = test_config(dir.path());
    let store = Store::open(&cfg).unwrap();
    let app = App::new(cfg, store).unwrap();
    let router = app.router();
    let (_, created) = json(
        router.clone(),
        "POST",
        "/v1/conversations",
        Some(json!({ "identity": { "e164": "+14155550111" } })),
    )
    .await;
    let cid = created["id"].clone();
    let (status, inbound) = json(
        router.clone(),
        "POST",
        "/v1/test/sms/inbound",
        Some(json!({ "from": "+14155550111", "body": "running late" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{inbound}");
    assert_eq!(inbound["medium"], "sms");
    assert_eq!(inbound["conversation_id"], cid);
}

#[tokio::test]
async fn outbound_sms_without_identity_is_forbidden() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = test_config(dir.path());
    let store = Store::open(&cfg).unwrap();
    let app = App::new(cfg, store).unwrap();
    let err = parley::sms::outbound(
        &app.state,
        "ten_local",
        "conv_missing",
        "+14155550111",
        "hi",
    )
    .await;
    assert!(err.is_err());
}

#[tokio::test]
async fn outbound_sms_with_identity_sends() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = test_config(dir.path());
    let store = Store::open(&cfg).unwrap();
    let app = App::new(cfg, store).unwrap();
    let router = app.router();
    let (_, created) = json(
        router,
        "POST",
        "/v1/conversations",
        Some(json!({ "identity": { "e164": "+14155550111" } })),
    )
    .await;
    let cid = created["id"].as_str().unwrap();
    let row = parley::sms::outbound(
        &app.state,
        "ten_local",
        cid,
        "+14155550111",
        "You're confirmed.",
    )
    .await
    .expect("send");
    assert_eq!(row.medium, "sms");
}
