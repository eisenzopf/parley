//! Desk pickup after widget Talk: operator UCTP voice offer, customer connection id stable.

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
use rvoip_uctp::payloads::connection::{ConnectionOffer, StreamOffer};
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
    cfg.operator_bootstrap_token = "bootstrap".into();
    cfg
}

async fn json_request(
    router: axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
    bearer: &str,
) -> (StatusCode, Value) {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {bearer}"))
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
    device: &str,
) {
    client
        .send(UctpEnvelope {
            v: 1,
            msg_type: MessageType::AuthHello,
            id: format!("env_hello_{device}"),
            ts: Utc::now(),
            cid: None,
            sid: None,
            connid: None,
            in_reply_to: None,
            payload: serde_json::to_value(auth::AuthHello {
                device: auth::Device {
                    id: device.into(),
                    kind: "browser".into(),
                    platform: "test".into(),
                    sdk_version: "parley-test/0.1".into(),
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
            id: format!("env_response_{device}"),
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

#[tokio::test]
async fn desk_pickup_operator_uctp_offer_keeps_customer_connection() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = test_config(dir.path());
    let store = Store::open(&cfg).expect("store");
    let app = App::new(cfg, store).expect("app");
    let uctp_addr = app.start_uctp().await.expect("uctp");
    let router = app.router();

    let (status, created) = json_request(
        router.clone(),
        "POST",
        "/v1/conversations",
        Some(json!({ "identity": { "visitor_id": "usr_desk" } })),
        "dev-only",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let cid = created["id"].as_str().expect("cid").to_string();

    let (status, voice) = json_request(
        router.clone(),
        "POST",
        &format!("/v1/conversations/{cid}/sessions"),
        Some(json!({ "medium": "voice", "direction": "inbound" })),
        "dev-only",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{voice}");
    let sid = voice["id"].as_str().expect("sid").to_string();
    let customer_conn = format!("conn_customer_{sid}");

    let (status, widget_tok) = json_request(
        router.clone(),
        "POST",
        "/v1/widget/tokens",
        Some(json!({ "visitor_id": "usr_desk", "origin": "http://127.0.0.1" })),
        "dev-only",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{widget_tok}");
    let widget_token = widget_tok["token"].as_str().expect("token").to_string();

    let url = Url::parse(&format!("ws://{uctp_addr}")).expect("url");
    let widget = UctpWsClient::connect(&url).await.expect("widget connect");
    let mut widget_in = widget.take_inbound().expect("widget inbound");
    auth_client(&widget, &mut widget_in, &widget_token, "dev_widget").await;
    widget
        .send(
            UctpEnvelope::new(
                MessageType::ConversationCreate,
                serde_json::to_value(ConversationCreate {
                    tenant_id: "ten_local".into(),
                    policy: ConversationPolicy::Persistent,
                    idle_close_secs: None,
                    metadata: serde_json::json!({ "visitor_id": "usr_desk" }),
                    initial_participants: vec![],
                })
                .unwrap(),
            )
            .with_cid(cid.clone()),
        )
        .await
        .expect("widget create");
    let opened = tokio::time::timeout(Duration::from_secs(5), widget_in.recv())
        .await
        .expect("opened timeout")
        .expect("opened");
    assert_eq!(opened.msg_type, MessageType::ConversationOpened);
    widget
        .send(
            UctpEnvelope::new(
                MessageType::SessionInvite,
                serde_json::to_value(SessionInvite {
                    from: "part_widget".into(),
                    to: vec!["part_ai".into()],
                    medium: "voice".into(),
                    intent: "talk".into(),
                    capabilities_offer: serde_json::Value::Object(Default::default()),
                })
                .unwrap(),
            )
            .with_cid(cid.clone())
            .with_sid(sid.clone()),
        )
        .await
        .expect("widget invite");

    let (status, boot) = json_request(
        router.clone(),
        "POST",
        "/v1/operators/bootstrap",
        Some(json!({ "token": "bootstrap", "email": "desk@parley.local" })),
        "dev-only",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{boot}");
    let op_token = boot["session_token"].as_str().expect("session").to_string();

    let (status, pickup) = json_request(
        router.clone(),
        "POST",
        &format!("/v1/conversations/{cid}/pickup/accept"),
        Some(json!({ "session_id": sid })),
        &op_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{pickup}");
    let op_conn = pickup["operator_connection_id"]
        .as_str()
        .expect("operator conn")
        .to_string();
    let op_pid = pickup["operator_participant_id"]
        .as_str()
        .expect("operator pid")
        .to_string();
    assert_eq!(op_conn, format!("conn_operator_{sid}"));

    let operator = UctpWsClient::connect(&url).await.expect("operator connect");
    let mut op_in = operator.take_inbound().expect("operator inbound");
    auth_client(&operator, &mut op_in, &op_token, "dev_desk").await;
    operator
        .send(
            UctpEnvelope::new(
                MessageType::ConversationCreate,
                serde_json::to_value(ConversationCreate {
                    tenant_id: "ten_local".into(),
                    policy: ConversationPolicy::Persistent,
                    idle_close_secs: None,
                    metadata: serde_json::json!({}),
                    initial_participants: vec![],
                })
                .unwrap(),
            )
            .with_cid(cid.clone()),
        )
        .await
        .expect("operator create");
    let _ = tokio::time::timeout(Duration::from_secs(5), op_in.recv()).await;
    operator
        .send(
            UctpEnvelope::new(
                MessageType::SessionInvite,
                serde_json::to_value(SessionInvite {
                    from: op_pid.clone(),
                    to: vec!["part_customer".into()],
                    medium: "voice".into(),
                    intent: "pickup".into(),
                    capabilities_offer: serde_json::Value::Object(Default::default()),
                })
                .unwrap(),
            )
            .with_cid(cid.clone())
            .with_sid(sid.clone()),
        )
        .await
        .expect("operator invite");
    operator
        .send(
            UctpEnvelope::new(
                MessageType::ConnectionOffer,
                serde_json::to_value(ConnectionOffer {
                    by_participant: op_pid,
                    substrate: "websocket".into(),
                    capabilities: serde_json::Value::Object(Default::default()),
                    streams_offered: vec![StreamOffer {
                        id: "strm_op_audio".into(),
                        kind: "audio".into(),
                        direction: "send-recv".into(),
                        codec_preferences: vec!["opus".into()],
                    }],
                    substrate_setup: serde_json::Value::Null,
                })
                .unwrap(),
            )
            .with_cid(cid.clone())
            .with_sid(sid.clone())
            .with_connid(op_conn.clone()),
        )
        .await
        .expect("operator offer");

    let (status, conns) = json_request(
        router.clone(),
        "GET",
        &format!("/v1/sessions/{sid}/connections"),
        None,
        "dev-only",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{conns}");
    let ids: Vec<&str> = conns["connections"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["id"].as_str())
        .collect();
    assert!(
        ids.contains(&customer_conn.as_str()),
        "customer connection must stay {customer_conn}: {conns}"
    );
    assert!(
        ids.contains(&op_conn.as_str()),
        "operator connection {op_conn} missing: {conns}"
    );

    let parts = app
        .state
        .store
        .list_participants("ten_local", &cid)
        .unwrap();
    assert!(parts.iter().any(|p| p.kind == "ai" && p.role == "observer"));
    assert!(parts.iter().any(|p| p.kind == "human" && p.role == "agent"));
    assert!(parts.iter().any(|p| p.kind == "system" && p.role == "observer"));
}
