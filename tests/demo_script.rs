//! PRD §8 laptop path with fakes — chat, SMS, close, vCon.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use parley::config::Config;
use parley::store::Store;
use parley::vapi_voice;
use parley::App;
use serde_json::{json, Value};
use tower::ServiceExt;

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
        builder
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
    };
    let response = router.oneshot(req).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value)
}

#[tokio::test]
async fn laptop_demo_chat_sms_close_vcon() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::default();
    cfg.sqlite_path = dir.path().join("parley.sqlite").display().to_string();
    cfg.blob_dir = dir.path().join("blobs").display().to_string();
    cfg.api_secret = "dev-only".into();
    cfg.vapi_chat_mode = "fake".into();
    let store = Store::open(&cfg).unwrap();
    let app = App::new(cfg, store).unwrap();
    let router = app.router();

    let (status, created) = json(
        router.clone(),
        "POST",
        "/v1/conversations",
        Some(json!({ "identity": { "e164": "+14155550111", "visitor_id": "usr_demo" } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let cid = created["id"].as_str().unwrap().to_string();
    let ids = vapi_voice::ai_and_customer(&app.state, "ten_local", &cid).unwrap();
    assert_ne!(ids.ai_participant_id, ids.customer_participant_id);
    assert!(created["participants"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["kind"] == "ai"));

    let (status, chat) = json(
        router.clone(),
        "POST",
        &format!("/v1/conversations/{cid}/messages"),
        Some(json!({ "medium": "chat", "body": "what are your hours?" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{chat}");
    assert_eq!(chat["medium"], "chat");

    let (status, listed) = json(
        router.clone(),
        "GET",
        &format!("/v1/conversations/{cid}/messages"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    let messages = listed["messages"].as_array().expect("messages");
    assert!(
        messages
            .iter()
            .any(|m| m["body"].as_str().unwrap_or("").starts_with("fake-reply:")),
        "AI chat reply missing: {listed}"
    );

    let (status, sms) = json(
        router.clone(),
        "POST",
        "/v1/test/sms/inbound",
        Some(json!({ "from": "+14155550111", "body": "also via sms" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{sms}");
    assert_eq!(sms["conversation_id"], cid);

    let (status, voice) = json(
        router.clone(),
        "POST",
        &format!("/v1/conversations/{cid}/sessions"),
        Some(json!({ "medium": "voice", "direction": "inbound" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{voice}");
    assert_eq!(voice["medium"], "voice");
    assert_eq!(voice["conversation_id"], cid);
    let sid = voice["id"].as_str().unwrap().to_string();

    let (status, pickup) = json(
        router.clone(),
        "POST",
        &format!("/v1/conversations/{cid}/pickup/accept"),
        Some(json!({ "session_id": sid })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{pickup}");
    let parts = app
        .state
        .store
        .list_participants("ten_local", &cid)
        .unwrap();
    assert!(parts.iter().any(|p| p.kind == "human" && p.role == "agent"));
    assert!(parts.iter().any(|p| p.kind == "ai" && p.role == "observer"));

    let (status, conns) = json(
        router.clone(),
        "GET",
        &format!("/v1/sessions/{sid}/connections"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{conns}");
    let customer_conn = conns["connections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"].as_str().unwrap().starts_with("conn_customer_"))
        .cloned()
        .expect("customer connection");
    let (status, conns_again) = json(
        router.clone(),
        "GET",
        &format!("/v1/sessions/{sid}/connections"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{conns_again}");
    assert_eq!(
        conns_again["connections"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == customer_conn["id"])
            .unwrap()["id"],
        customer_conn["id"]
    );

    let (status, closed) = json(
        router.clone(),
        "POST",
        &format!("/v1/conversations/{cid}/close"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{closed}");

    let (status, vcon) = json(
        router,
        "GET",
        &format!("/v1/conversations/{cid}/vcon"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{vcon}");
    assert!(vcon.get("parties").is_some() || vcon.get("vcon").is_some());
    assert!(vcon
        .pointer("/parley.vcon.conversation.v1/cid")
        .and_then(|v| v.as_str())
        .is_some());
}
