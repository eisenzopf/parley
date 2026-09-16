use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use parley::config::Config;
use parley::store::Store;
use parley::App;
use serde_json::{json, Value};
use tower::ServiceExt;

#[tokio::test]
async fn request_human_writes_pickup_event() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::default();
    cfg.sqlite_path = dir.path().join("parley.sqlite").display().to_string();
    cfg.blob_dir = dir.path().join("blobs").display().to_string();
    cfg.api_secret = "dev-only".into();
    let store = Store::open(&cfg).unwrap();
    let app = App::new(cfg, store).unwrap();
    let router = app.router();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/conversations")
        .header("authorization", "Bearer dev-only")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({ "identity": { "e164": "+14155550111" } })).unwrap(),
        ))
        .unwrap();
    let created: Value = serde_json::from_slice(
        &router
            .clone()
            .oneshot(req)
            .await
            .unwrap()
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes(),
    )
    .unwrap();
    let cid = created["id"].as_str().unwrap();
    let tool = Request::builder()
        .method("POST")
        .uri("/v1/vapi/tools")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({
                "name": "request_human",
                "parameters": { "conversation_id": cid }
            }))
            .unwrap(),
        ))
        .unwrap();
    let response = router.clone().oneshot(tool).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let events = app
        .state
        .store
        .list_events("ten_local", cid)
        .unwrap();
    assert!(events.iter().any(|e| e.event_type == "pickup.requested"));
}
