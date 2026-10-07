//! Phase 3 — REST-first conversation, then UCTP envelopes on the widget socket.

use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::Utc;
use http_body_util::BodyExt;
use parley::config::Config;
use parley::store::Store;
use parley::App;
use rvoip_uctp::envelope::UctpEnvelope;
use rvoip_uctp::payloads::auth;
use rvoip_uctp::payloads::conversation::{ConversationCreate, ConversationPolicy};
use rvoip_uctp::payloads::session::SessionInvite;
use rvoip_uctp::types::MessageType;
use rvoip_websocket::UctpWsClient;
use serde_json::{json, Value};
use tower::ServiceExt;
use url::Url;

fn test_config(dir: &std::path::Path) -> Config {
    let mut cfg = Config::default();
    cfg.sqlite_path = dir.join("parley.sqlite").display().to_string();
    cfg.blob_dir = dir.join("blobs").display().to_string();
    cfg.api_secret = "dev-only".into();
    cfg.tenant_id = "ten_local".into();
    cfg.bind_uctp_ws = "127.0.0.1:0".into();
    cfg
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
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

async fn auth_client(
    client: &UctpWsClient,
    inbound: &mut tokio::sync::mpsc::Receiver<UctpEnvelope>,
    token: &str,
) {
    client
        .send(UctpEnvelope {
            v: 1,
            msg_type: MessageType::AuthHello,
            id: "env_hello".into(),
            ts: Utc::now(),
            cid: None,
            sid: None,
            connid: None,
            in_reply_to: None,
            payload: serde_json::to_value(auth::AuthHello {
                device: auth::Device {
                    id: "dev_widget".into(),
                    kind: "browser".into(),
                    platform: "test".into(),
                    sdk_version: "parley-widget/0.1".into(),
                },
                auth_methods: vec!["bearer".into()],
                capabilities: serde_json::Value::Object(Default::default()),
            })
            .unwrap(),
            signature: None,
        })
        .await
        .expect("hello");
    let challenge = tokio::time::timeout(Duration::from_secs(5), inbound.recv())
        .await
        .expect("challenge timeout")
        .expect("challenge");
    assert_eq!(challenge.msg_type, MessageType::AuthChallenge);
    client
        .send(UctpEnvelope {
            v: 1,
            msg_type: MessageType::AuthResponse,
            id: "env_response".into(),
            ts: Utc::now(),
            cid: None,
            sid: None,
            connid: None,
            in_reply_to: Some(challenge.id),
            payload: serde_json::to_value(auth::AuthResponse {
                method: "bearer".into(),
                credential: token.into(),
                actor_token: None,
            })
            .unwrap(),
            signature: None,
        })
        .await
        .expect("response");
    let session = tokio::time::timeout(Duration::from_secs(5), inbound.recv())
        .await
        .expect("auth.session timeout")
        .expect("auth.session");
    assert_eq!(session.msg_type, MessageType::AuthSession);
}

fn assert_uctp_type(msg: MessageType) {
    let wire = format!("{msg:?}");
    let _ = wire;
    let s = match msg {
        MessageType::Unknown(ref name) => name.as_str(),
        _ => "",
    };
    assert_ne!(s, "offer");
    assert_ne!(s, "answer");
}

#[tokio::test]
async fn rest_create_then_uctp_text_session_and_message() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = test_config(dir.path());
    let store = Store::open(&cfg).expect("store");
    let app = App::new(cfg, store).expect("app");
    let uctp_addr = app.start_uctp().await.expect("uctp");
    let router = app.router();

    // An unrelated open Conversation must never capture this socket's data.
    let (_, other) = json_request(
        router.clone(),
        "POST",
        "/v1/conversations",
        Some(json!({ "identity": { "visitor_id": "someone_else" } })),
    )
    .await;

    let (status, created) = json_request(
        router.clone(),
        "POST",
        "/v1/conversations",
        Some(json!({ "identity": { "visitor_id": "usr_widget" } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let cid = created["id"].as_str().expect("cid").to_string();

    let (status, token_body) = json_request(
        router.clone(),
        "POST",
        "/v1/widget/tokens",
        Some(json!({ "visitor_id": "usr_widget", "origin": "http://127.0.0.1" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{token_body}");
    let token = token_body["token"].as_str().expect("token").to_string();

    let url = Url::parse(&format!("ws://{uctp_addr}")).expect("url");
    let client = UctpWsClient::connect(&url).await.expect("connect");
    let mut inbound = client.take_inbound().expect("inbound");
    auth_client(&client, &mut inbound, &token).await;

    client
        .send(
            UctpEnvelope::new(
                MessageType::ConversationCreate,
                serde_json::to_value(ConversationCreate {
                    tenant_id: "ten_local".into(),
                    policy: ConversationPolicy::Persistent,
                    idle_close_secs: None,
                    metadata: serde_json::json!({ "visitor_id": "usr_widget" }),
                    initial_participants: vec![],
                })
                .unwrap(),
            )
            .with_cid(cid.clone()),
        )
        .await
        .expect("create");
    let opened = tokio::time::timeout(Duration::from_secs(5), inbound.recv())
        .await
        .expect("opened timeout")
        .expect("opened");
    assert_eq!(opened.msg_type, MessageType::ConversationOpened);
    assert_eq!(opened.cid.as_deref(), Some(cid.as_str()));
    assert_uctp_type(opened.msg_type);

    let sid = "sess_widget_text";
    client
        .send(
            UctpEnvelope::new(
                MessageType::SessionInvite,
                serde_json::to_value(SessionInvite {
                    from: "part_widget".into(),
                    to: vec!["part_ai".into()],
                    medium: "text".into(),
                    intent: "message".into(),
                    capabilities_offer: serde_json::Value::Object(Default::default()),
                })
                .unwrap(),
            )
            .with_cid(cid.clone())
            .with_sid(sid),
        )
        .await
        .expect("invite");

    let (status, session) = json_request(
        router.clone(),
        "POST",
        &format!("/v1/conversations/{cid}/sessions"),
        Some(json!({ "medium": "text", "direction": "inbound" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{session}");
    assert_eq!(session["medium"], "text");

    let wire_conn = "conn_widget_text";
    client
        .send(
            UctpEnvelope::new(
                MessageType::ConnectionOffer,
                serde_json::to_value(rvoip_uctp::payloads::connection::ConnectionOffer {
                    by_participant: "part_widget".into(),
                    substrate: "websocket".into(),
                    capabilities: serde_json::Value::Object(Default::default()),
                    streams_offered: vec![rvoip_uctp::payloads::connection::StreamOffer {
                        id: "strm_widget_data".into(),
                        kind: "audio".into(),
                        direction: "sendrecv".into(),
                        codec_preferences: vec!["opus".into()],
                    }],
                    substrate_setup: serde_json::Value::Null,
                })
                .unwrap(),
            )
            .with_sid(sid)
            .with_connid(wire_conn),
        )
        .await
        .expect("connection.offer");

    client
        .send(
            UctpEnvelope::new(
                MessageType::MessageSend,
                serde_json::json!({
                    "msg_id": "msg_widget_1",
                    "from": "part_widget",
                    "to": "all",
                    "content_type": "text/plain",
                    "label": "rvoip-messages",
                    "body": "hello from uctp",
                    "body_encoding": "utf8",
                    "attachments": []
                }),
            )
            .with_cid(cid.clone())
            .with_sid(sid)
            .with_connid(wire_conn),
        )
        .await
        .expect("message.send");

    let mut found = false;
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let (_, listed) = json_request(
            router.clone(),
            "GET",
            &format!("/v1/conversations/{cid}/messages"),
            None,
        )
        .await;
        if listed["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|m| m["body"] == "hello from uctp" && m["medium"] == "chat")
        {
            found = true;
            break;
        }
    }
    assert!(found, "uctp message.send should persist as chat");
    let (_, unrelated) = json_request(
        router.clone(),
        "GET",
        &format!(
            "/v1/conversations/{}/messages",
            other["id"].as_str().unwrap()
        ),
        None,
    )
    .await;
    assert!(unrelated["messages"].as_array().unwrap().is_empty());
    let (_, actual) = json_request(
        router.clone(),
        "GET",
        &format!("/v1/conversations/{cid}/messages"),
        None,
    )
    .await;
    let customer = created["participants"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["role"] == "customer")
        .unwrap();
    let message = actual["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["body"] == "hello from uctp")
        .unwrap();
    assert_eq!(message["from_participant"], customer["id"]);

    let (_, sessions) = json_request(
        router,
        "GET",
        &format!("/v1/conversations/{cid}/sessions"),
        None,
    )
    .await;
    assert!(
        sessions["sessions"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|s| s["medium"] == "text"),
        "{sessions}"
    );
}

#[tokio::test]
async fn uctp_first_identity_reuses_sqlite_cid() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = test_config(dir.path());
    let store = Store::open(&cfg).expect("store");
    let app = App::new(cfg, store).expect("app");
    let uctp_addr = app.start_uctp().await.expect("uctp");
    let router = app.router();

    let (status, token_body) = json_request(
        router.clone(),
        "POST",
        "/v1/widget/tokens",
        Some(json!({ "visitor_id": "usr_uctp_first", "origin": "http://127.0.0.1" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{token_body}");
    let token = token_body["token"].as_str().expect("token").to_string();

    let url = Url::parse(&format!("ws://{uctp_addr}")).expect("url");
    let client = UctpWsClient::connect(&url).await.expect("connect");
    let mut inbound = client.take_inbound().expect("inbound");
    auth_client(&client, &mut inbound, &token).await;

    let create = ConversationCreate {
        tenant_id: "ten_local".into(),
        policy: ConversationPolicy::Persistent,
        idle_close_secs: None,
        metadata: serde_json::json!({ "visitor_id": "usr_uctp_first" }),
        initial_participants: vec![],
    };
    client
        .send(UctpEnvelope::new(
            MessageType::ConversationCreate,
            serde_json::to_value(&create).unwrap(),
        ))
        .await
        .expect("create");
    let opened = tokio::time::timeout(Duration::from_secs(5), inbound.recv())
        .await
        .expect("opened timeout")
        .expect("opened");
    assert_eq!(opened.msg_type, MessageType::ConversationOpened);
    let cid = opened.cid.clone().expect("assigned cid");

    client
        .send(UctpEnvelope::new(
            MessageType::ConversationCreate,
            serde_json::to_value(&create).unwrap(),
        ))
        .await
        .expect("second create");
    let opened_again = tokio::time::timeout(Duration::from_secs(5), inbound.recv())
        .await
        .expect("second opened timeout")
        .expect("second opened");
    assert_eq!(opened_again.msg_type, MessageType::ConversationOpened);
    assert_eq!(opened_again.cid.as_deref(), Some(cid.as_str()));

    let (status, conv) =
        json_request(router, "GET", &format!("/v1/conversations/{cid}"), None).await;
    assert_eq!(status, StatusCode::OK, "{conv}");
    assert_eq!(conv["id"], cid);
    assert_eq!(conv["match_kind"], "get");
}
