use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use parley::config::Config;
use parley::store::Store;
use parley::vapi_chat;
use parley::App;
use serde_json::{json, Value};
use tower::ServiceExt;

fn test_config(dir: &std::path::Path) -> Config {
    let mut cfg = Config::default();
    cfg.sqlite_path = dir.join("parley.sqlite").display().to_string();
    cfg.blob_dir = dir.join("blobs").display().to_string();
    cfg.api_secret = "dev-only".into();
    cfg.vapi_chat_mode = "fake".into();
    cfg
}

#[tokio::test]
async fn fake_vapi_chat_persists_ai_reply() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = test_config(dir.path());
    let store = Store::open(&cfg).expect("store");
    let app = App::new(cfg, store).expect("app");
    let router = app.router();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/conversations")
        .header("authorization", "Bearer dev-only")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({ "identity": { "visitor_id": "usr_chat" } })).unwrap(),
        ))
        .unwrap();
    let response = router.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let created: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let cid = created["id"].as_str().unwrap();
    let reply = vapi_chat::complete(&app.state, &app.state.config.tenant_id, cid, "hours?")
        .await
        .expect("fake chat");
    assert!(reply.text.starts_with("fake-reply:"));
    let msg = parley::conversation::post_message(
        &app.state,
        &app.state.config.tenant_id,
        cid,
        parley::conversation::PostMessage {
            medium: "chat".into(),
            sender_participant_id: None,
            body: reply.text,
            inbound: false,
        },
        None,
    )
    .expect("store ai message");
    assert_eq!(msg.medium, "chat");
    assert!(!msg.medium.contains("sms"));
}
