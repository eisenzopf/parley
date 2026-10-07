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
    cfg.tenant_id = "ten_local".into();
    cfg
}

async fn app() -> (axum::Router, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = test_config(dir.path());
    let store = Store::open(&cfg).expect("store");
    let app = App::new(cfg, store).expect("app");
    (app.router(), dir)
}

async fn json_request(
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
    let response = router.oneshot(req).await.expect("oneshot");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or(Value::String(String::from_utf8_lossy(&bytes).into()))
    };
    (status, value)
}

#[tokio::test]
async fn post_then_get_same_conversation() {
    let (router, _dir) = app().await;
    let (status, created) = json_request(
        router.clone(),
        "POST",
        "/v1/conversations",
        Some(json!({ "identity": { "e164": "+14155550111" } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let cid = created["id"].as_str().expect("id");
    assert!(cid.starts_with("conv_"));
    let (status, got) =
        json_request(router, "GET", &format!("/v1/conversations/{cid}"), None).await;
    assert_eq!(status, StatusCode::OK, "{got}");
    assert_eq!(got["id"], created["id"]);
}

#[tokio::test]
async fn second_post_same_e164_continues() {
    let (router, _dir) = app().await;
    let body = json!({ "identity": { "e164": "+14155550111" } });
    let (_, first) = json_request(
        router.clone(),
        "POST",
        "/v1/conversations",
        Some(body.clone()),
    )
    .await;
    let (_, second) = json_request(router, "POST", "/v1/conversations", Some(body)).await;
    assert_eq!(first["id"], second["id"]);
    assert_eq!(second["match_kind"], "continue");
}

#[tokio::test]
async fn sms_messages_round_trip() {
    let (router, _dir) = app().await;
    let (_, created) = json_request(
        router.clone(),
        "POST",
        "/v1/conversations",
        Some(json!({ "identity": { "e164": "+14155550111" } })),
    )
    .await;
    let cid = created["id"].as_str().unwrap();
    let (status, posted) = json_request(
        router.clone(),
        "POST",
        &format!("/v1/conversations/{cid}/messages"),
        Some(json!({ "medium": "sms", "body": "You're confirmed for Friday 3pm." })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{posted}");
    assert_eq!(posted["medium"], "sms");
    let (status, listed) = json_request(
        router,
        "GET",
        &format!("/v1/conversations/{cid}/messages"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(listed["messages"][0]["medium"], "sms");
    assert_eq!(listed["messages"][0]["body"], posted["body"]);
}

#[tokio::test]
async fn timeline_mixes_session_and_message_without_flattening() {
    let (router, _dir) = app().await;
    let (_, created) = json_request(
        router.clone(),
        "POST",
        "/v1/conversations",
        Some(json!({ "identity": { "e164": "+14155550111" } })),
    )
    .await;
    let cid = created["id"].as_str().unwrap();
    let (status, session) = json_request(
        router.clone(),
        "POST",
        &format!("/v1/conversations/{cid}/sessions"),
        Some(json!({ "medium": "text", "direction": "inbound" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let _ = json_request(
        router.clone(),
        "POST",
        &format!("/v1/conversations/{cid}/messages"),
        Some(json!({ "medium": "sms", "body": "hello from sms" })),
    )
    .await;
    let (status, timeline) = json_request(
        router,
        "GET",
        &format!("/v1/conversations/{cid}/timeline"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{timeline}");
    let items = timeline["items"].as_array().expect("items");
    assert!(items
        .iter()
        .any(|i| i["type"] == "session" && i.get("medium").is_some()));
    assert!(items
        .iter()
        .any(|i| i["type"] == "message" && i.get("body").is_some()));
    assert!(items.iter().all(|i| i.get("call").is_none()));
    let session_item = items.iter().find(|i| i["type"] == "session").unwrap();
    let message_item = items.iter().find(|i| i["type"] == "message").unwrap();
    assert!(session_item.get("body").is_none());
    assert!(message_item.get("started_at").is_none());
}

#[tokio::test]
async fn healthz_reports_sqlite_and_blob() {
    let (router, _dir) = app().await;
    let (status, body) = json_request(router, "GET", "/healthz", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["sqlite"], true);
    assert_eq!(body["blob_writable"], true);
    assert!(body.get("uctp").is_some());
    assert!(body.get("sip").is_some());
}

#[tokio::test]
async fn public_widget_token_does_not_need_api_secret() {
    let (router, _dir) = app().await;
    let req = Request::builder()
        .method("POST")
        .uri("/v1/public/widget-token")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({ "visitor_id": "usr_public" })).unwrap(),
        ))
        .unwrap();
    let response = router.oneshot(req).await.expect("oneshot");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["token"].as_str().unwrap().starts_with("p1."));
}

#[tokio::test]
async fn public_config_has_uctp_and_no_secrets() {
    let (router, _dir) = app().await;
    let (status, body) = json_request(router, "GET", "/v1/public", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["uctp_ws_url"].as_str().unwrap().starts_with("ws://"));
    assert!(body["http_base"].as_str().unwrap().contains("127.0.0.1"));
    let dumped = body.to_string();
    assert!(!dumped.contains("sk-"));
    assert!(!dumped.contains("KEY0"));
}

#[tokio::test]
async fn no_route_named_call() {
    let (router, _dir) = app().await;
    for path in [
        "/v1/calls",
        "/v1/legs",
        "/v1/dialogs",
        "/v1/bots",
        "/v1/tickets",
    ] {
        let (status, _) = json_request(router.clone(), "GET", path, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn events_sse_requires_auth() {
    let (router, _dir) = app().await;
    let req = Request::builder()
        .method("GET")
        .uri("/v1/events")
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(req).await.expect("oneshot");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn events_sse_pushes_conversation_opened() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = test_config(dir.path());
    let store = Store::open(&cfg).expect("store");
    let app = App::new(cfg, store).expect("app");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        axum::serve(listener, app.router()).await.ok();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let client = reqwest::Client::new();
    let mut sse = client
        .get(format!("http://{addr}/v1/events?access_token=dev-only"))
        .header("accept", "text/event-stream")
        .send()
        .await
        .expect("sse connect");
    assert!(sse.status().is_success(), "{}", sse.status());
    let created = client
        .post(format!("http://{addr}/v1/conversations"))
        .header("authorization", "Bearer dev-only")
        .header("content-type", "application/json")
        .json(&json!({ "identity": { "visitor_id": "usr_sse" } }))
        .send()
        .await
        .expect("create");
    assert!(created.status().is_success(), "{}", created.status());
    let mut body = String::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(std::time::Duration::from_millis(400), sse.chunk()).await {
            Ok(Ok(Some(chunk))) => {
                body.push_str(&String::from_utf8_lossy(&chunk));
                if body.contains("conversation.opened") {
                    break;
                }
            }
            Ok(Ok(None)) | Ok(Err(_)) => break,
            Err(_) => continue,
        }
    }
    assert!(
        body.contains("event: conversation.opened"),
        "sse body was {body:?}"
    );
}
