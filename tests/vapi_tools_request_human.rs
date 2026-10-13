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
    let events = app.state.store.list_events("ten_local", cid).unwrap();
    assert!(events.iter().any(|e| e.event_type == "pickup.requested"));
}

#[tokio::test]
async fn realtime_conference_tool_webhook_acknowledges_nested_call_without_routing() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::default();
    cfg.sqlite_path = dir.path().join("parley.sqlite").display().to_string();
    cfg.blob_dir = dir.path().join("blobs").display().to_string();
    cfg.vapi_api_key = "fixture-secret".into();
    let app = App::new(cfg.clone(), Store::open(&cfg).unwrap()).unwrap();
    let router = app.router();
    let body = json!({"message":{"type":"tool-calls","toolCallList":[
        {"id":"call_join","function":{"name":"request_browser_join","arguments":{}}},
        {"id":"call_finish","function":{"name":"finish_call","arguments":{"reason":"completed"}}},
        {"id":"call_bad","function":{"name":"finish_call","arguments":{"reason":"transfer"}}}
    ]}});
    let sid = "sess_fixture";
    let request = |signature: &str| {
        Request::builder()
            .method("POST")
            .uri(format!("/v1/vapi/conference-tools/{sid}/{signature}"))
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap()
    };
    assert_eq!(
        router
            .clone()
            .oneshot(request("wrong"))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let signature = parley::vapi_tools::conference_tool_signature("fixture-secret", sid);
    let response = router.oneshot(request(&signature)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let value: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(
        value,
        json!({"results":[
            {"toolCallId":"call_join","result":"Received."},
            {"toolCallId":"call_finish","result":"Received."},
            {"toolCallId":"call_bad","error":"Invalid conference action."}
        ]})
    );
}
