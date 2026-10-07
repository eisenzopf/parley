//! Talk: same cid, voice Session, UCTP `connection.offer` with WebRTC
//! `substrate_setup` (not a private `{type,sdp}` JSON).

#![cfg(feature = "media-webrtc")]

use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::Utc;
use http_body_util::BodyExt;
use parley::config::Config;
use parley::store::Store;
use parley::App;
use rvoip_core::events::Event;
use rvoip_uctp::envelope::UctpEnvelope;
use rvoip_uctp::payloads::auth;
use rvoip_uctp::payloads::connection::{ConnectionAnswer, ConnectionOffer, StreamOffer};
use rvoip_uctp::payloads::conversation::{ConversationCreate, ConversationPolicy};
use rvoip_uctp::payloads::session::SessionInvite;
use rvoip_uctp::types::MessageType;
use rvoip_websocket::{UctpWsClient, WebRtcMediaBridge};
use serde_json::{json, Value};
use tower::ServiceExt;
use url::Url;

fn install_crypto_provider() {
    parley::tls::ensure_rustls_ring();
}

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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn talk_same_cid_connection_offer_webrtc() {
    install_crypto_provider();
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = test_config(dir.path());
    let store = Store::open(&cfg).expect("store");
    let app = App::new(cfg, store).expect("app");
    let mut events = app.state.orchestrator.subscribe_events();
    let uctp_addr = app.start_uctp().await.expect("uctp");
    let router = app.router();

    let (status, created) = json_request(
        router.clone(),
        "POST",
        "/v1/conversations",
        Some(json!({ "identity": { "visitor_id": "usr_talk" } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let cid = created["id"].as_str().expect("cid").to_string();

    let (status, token_body) = json_request(
        router.clone(),
        "POST",
        "/v1/widget/tokens",
        Some(json!({ "visitor_id": "usr_talk", "origin": "http://127.0.0.1" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{token_body}");
    let token = token_body["token"].as_str().expect("token").to_string();

    let (status, voice) = json_request(
        router.clone(),
        "POST",
        &format!("/v1/conversations/{cid}/sessions"),
        Some(json!({ "medium": "voice", "direction": "inbound" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{voice}");
    let sid = voice["id"].as_str().expect("sid").to_string();
    let connid = format!("conn_customer_{sid}");

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
                    metadata: serde_json::json!({ "visitor_id": "usr_talk" }),
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

    client
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
        .expect("invite");

    let _ = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match events.recv().await {
                Ok(Event::ConnectionInbound { .. }) => break,
                Ok(_) => continue,
                Err(_) => break,
            }
        }
    })
    .await;

    let offerer = std::sync::Arc::new(
        WebRtcMediaBridge::new_offerer()
            .await
            .expect("offerer bridge"),
    );
    let offer_setup = offerer
        .local_substrate_setup()
        .await
        .expect("offerer local SDP");

    client
        .send(
            UctpEnvelope::new(
                MessageType::ConnectionOffer,
                serde_json::to_value(ConnectionOffer {
                    by_participant: "part_widget".into(),
                    substrate: "websocket+webrtc".into(),
                    capabilities: serde_json::Value::Object(Default::default()),
                    streams_offered: vec![StreamOffer {
                        id: "strm_audio".into(),
                        kind: "audio".into(),
                        direction: "send-recv".into(),
                        codec_preferences: vec!["opus".into()],
                    }],
                    substrate_setup: serde_json::to_value(offer_setup).unwrap(),
                })
                .unwrap(),
            )
            .with_cid(cid.clone())
            .with_sid(sid.clone())
            .with_connid(connid.clone()),
        )
        .await
        .expect("connection.offer");

    let answer_env = loop {
        let env = tokio::time::timeout(Duration::from_secs(15), inbound.recv())
            .await
            .expect("answer timeout")
            .expect("inbound closed");
        if env.msg_type == MessageType::ConnectionAnswer {
            break env;
        }
    };
    let answer_payload: ConnectionAnswer = answer_env.decode_payload().expect("decode answer");
    let setup: rvoip_uctp::payloads::connection::WebRtcSubstrateSetup =
        serde_json::from_value(answer_payload.substrate_setup)
            .expect("connection.answer must carry websocket+webrtc substrate_setup");
    assert_eq!(setup.kind, "websocket+webrtc");
    assert!(setup.sdp.starts_with("v="), "sdp {}", setup.sdp);

    offerer
        .set_remote_substrate_setup(setup)
        .await
        .expect("apply answer");
    offerer
        .wait_connected(Duration::from_secs(15))
        .await
        .expect("offerer connected");

    let (status, listed) = json_request(
        router.clone(),
        "GET",
        &format!("/v1/sessions/{sid}/connections"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    let ids: Vec<&str> = listed["connections"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["id"].as_str())
        .collect();
    assert!(
        ids.contains(&connid.as_str()),
        "customer connection id must stay {connid}: {listed}"
    );

    let (status, conv) =
        json_request(router, "GET", &format!("/v1/conversations/{cid}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(conv["id"], cid);
}
