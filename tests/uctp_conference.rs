#![cfg(feature = "uctp")]
use parley::store::conference::{InboundSmsOutcome, Member};
use parley::{store::Store, App, Config};
use rvoip_uctp::{envelope::UctpEnvelope, types::MessageType};
use rvoip_websocket::UctpWsClient;
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc;

#[cfg(all(feature = "sip", feature = "vapi", feature = "media-webrtc"))]
#[path = "support/conference_media.rs"]
mod conference_media;
#[cfg(all(feature = "sip", feature = "vapi", feature = "media-webrtc"))]
#[path = "support/conference_scenario.rs"]
mod conference_scenario;
#[cfg(all(feature = "sip", feature = "vapi", feature = "media-webrtc"))]
#[path = "support/conference_phone_media.rs"]
mod conference_phone_media;
#[cfg(all(feature = "sip", feature = "vapi", feature = "media-webrtc"))]
#[path = "support/vapi_provider.rs"]
mod vapi_provider;

#[cfg(feature = "sip")]
#[path = "support/conference_failures.rs"]
mod conference_failures;

struct Peer {
    client: Arc<UctpWsClient>,
    incoming: mpsc::Receiver<UctpEnvelope>,
}

impl Peer {
    async fn connect(url: &url::Url, token: &str) -> Self {
        let client = UctpWsClient::connect(url).await.unwrap();
        let incoming = client.take_inbound().unwrap();
        let mut peer = Self { client, incoming };
        peer.client.send(UctpEnvelope::new(MessageType::AuthHello,json!({"device":{"id":"dev_conference","kind":"desktop","platform":"test","sdk_version":"test"},"auth_methods":["bearer"],"capabilities":{}}))).await.unwrap();
        let challenge = peer.next().await;
        assert_eq!(challenge.msg_type, MessageType::AuthChallenge);
        assert_eq!(
            challenge.payload["server_capabilities"]["application_profiles"],
            json!(["conversation-control/1"])
        );
        peer.client
            .send(
                UctpEnvelope::new(
                    MessageType::AuthResponse,
                    json!({"method":"bearer","credential":token}),
                )
                .with_in_reply_to(challenge.id),
            )
            .await
            .unwrap();
        assert_eq!(peer.next().await.msg_type, MessageType::AuthSession);
        peer
    }
    async fn next(&mut self) -> UctpEnvelope {
        tokio::time::timeout(Duration::from_secs(3), self.incoming.recv())
            .await
            .unwrap()
            .unwrap()
    }
    async fn request(&mut self, request: UctpEnvelope) -> UctpEnvelope {
        let id = request.id.clone();
        self.client.send(request).await.unwrap();
        loop {
            let reply = self.next().await;
            if reply.in_reply_to.as_deref() == Some(&id) {
                return reply;
            }
        }
    }
}

fn command(kind: MessageType, cid: Option<&str>, mut payload: Value) -> UctpEnvelope {
    payload["profile"] = json!("conversation-control/1");
    let mut request = UctpEnvelope::new(kind, payload);
    request.cid = cid.map(str::to_string);
    request
}

fn config(path: &std::path::Path) -> Config {
    let mut cfg = Config::default();
    cfg.sqlite_path = path.join("test.sqlite").display().to_string();
    cfg.blob_dir = path.join("blobs").display().to_string();
    cfg.api_secret = "test-admin".into();
    cfg.vapi_chat_mode = "fake".into();
    cfg.bind_uctp_ws = "127.0.0.1:0".into();
    cfg.telnyx_from = "+14155550000".into();
    cfg
}

async fn provision(admin: &mut Peer) -> (String, Vec<Member>) {
    provision_with_sip(admin, "sip:booker@localhost").await
}

async fn provision_with_sip(admin: &mut Peer, sip: &str) -> (String, Vec<Member>) {
    let reply = admin
        .request(command(
            MessageType::ConversationCreate,
            None,
            json!({"participants":[
                {"alias":"jonathan","name":"Jonathan","role":"owner","sms":"+14155550101"},
                {"alias":"alex","name":"Alex","role":"companion","sms":"+14155550102"},
                {"alias":"booker","name":"Booker","role":"booker","sms":"+14155550103","sip":sip},
                {"alias":"organizer","name":"Organizer","role":"organizer","sms":"+14155550104"},
                {"alias":"assistant","name":"Vapi","role":"assistant"}
            ]}),
        ))
        .await;
    assert_eq!(
        reply.msg_type,
        MessageType::ConversationOpened,
        "{}",
        reply.payload
    );
    (
        reply.cid.unwrap(),
        serde_json::from_value(reply.payload["participants"].clone()).unwrap(),
    )
}

async fn member_peer(app: &App, url: &url::Url, cid: &str, member: &Member) -> Peer {
    let token = app
        .state
        .store
        .issue_conference_token("ten_local", cid, &member.participant_id)
        .unwrap();
    Peer::connect(url, &token).await
}

fn message(cid: &str, mid: &str, to: Vec<String>, body: &str) -> UctpEnvelope {
    command(
        MessageType::MessageSend,
        Some(cid),
        json!({"msg_id":mid,"from":"spoofed-sender","to":to,"content_type":"text/plain","delivery":"sms","body":body}),
    )
}

async fn wait_sent(app: &App, cid: &str, count: usize) {
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let deliveries = app
                .state
                .store
                .conference_deliveries("ten_local", cid)
                .unwrap();
            if deliveries.len() == count && deliveries.iter().all(|d| d.state == "sent") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn four_recipients_and_reply_use_one_conversation_without_voice() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let app = App::new(cfg.clone(), Store::open(&cfg).unwrap()).unwrap();
    let url = url::Url::parse(&format!("ws://{}", app.start_uctp().await.unwrap())).unwrap();
    let mut admin = Peer::connect(&url, "test-admin").await;
    let (cid, members) = provision(&mut admin).await;
    let mut worker = member_peer(&app, &url, &cid, &members[4]).await;
    for (i, recipient) in members[..4].iter().enumerate() {
        let reply = worker
            .request(message(
                &cid,
                &format!("msg_final_{i}"),
                vec![recipient.participant_id.clone()],
                &format!("Arrangements for {}", recipient.alias),
            ))
            .await;
        assert_eq!(reply.msg_type, MessageType::Ack, "{}", reply.payload);
        assert_eq!(reply.payload["message"]["from"], members[4].participant_id);
    }
    wait_sent(&app, &cid, 4).await;
    let deliveries = app
        .state
        .store
        .conference_deliveries("ten_local", &cid)
        .unwrap();
    for (d, m) in deliveries.iter().zip(&members[..4]) {
        assert_eq!(Some(&d.recipient_address), m.sms.as_ref());
        assert_eq!(d.participant_id, m.participant_id);
        assert!(d.body.contains(&m.alias));
        assert!(d.provider_id.as_ref().unwrap().starts_with("fake_"));
    }
    assert!(app
        .state
        .store
        .list_sessions("ten_local", &cid)
        .unwrap()
        .is_empty());
    let InboundSmsOutcome::Routed(reply) = app
        .state
        .store
        .receive_conference_sms(
            "ten_local",
            "provider_reply_1",
            "+14155550104",
            "+14155550000",
            "Terminal C instead",
        )
        .unwrap()
    else {
        panic!("expected routed SMS");
    };
    assert_eq!(reply.conversation_id, cid);
    assert_eq!(
        reply.from_participant.as_ref(),
        Some(&members[3].participant_id)
    );
    let InboundSmsOutcome::Routed(duplicate) = app
        .state
        .store
        .receive_conference_sms(
            "ten_local",
            "provider_reply_1",
            "+14155550104",
            "+14155550000",
            "Terminal C instead",
        )
        .unwrap()
    else {
        panic!("expected routed SMS");
    };
    assert_eq!(reply.id, duplicate.id);
    let history = worker
        .request(command(MessageType::MessageHistory, Some(&cid), json!({})))
        .await;
    assert_eq!(history.payload["messages"].as_array().unwrap().len(), 5);
    assert_eq!(history.payload["messages"][4]["body"], "Terminal C instead");
}

#[tokio::test]
async fn widget_named_api_cannot_provision_conference_members() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let token = parley::auth::mint_widget_token(&cfg, Some("api".into()), None).unwrap();
    let app = App::new(cfg.clone(), Store::open(&cfg).unwrap()).unwrap();
    let url = url::Url::parse(&format!("ws://{}", app.start_uctp().await.unwrap())).unwrap();
    let mut widget = Peer::connect(&url, &token).await;
    let reply = widget
        .request(command(
            MessageType::ConversationCreate,
            None,
            json!({"participants":[{"alias":"owner","name":"Owner","role":"owner"}]}),
        ))
        .await;
    assert_eq!(reply.msg_type, MessageType::Error);
    assert_eq!(reply.payload["code"], 403);
}

#[test]
fn provider_receipt_before_submission_response_survives_restart() {
    for final_state in ["delivered", "failed"] {
        let dir = tempfile::tempdir().unwrap();
        let cfg = config(dir.path());
        let store = Store::open(&cfg).unwrap();
        let members = store
            .create_conference(
                "ten_local",
                "conv_receipt",
                &[parley::store::conference::MemberInput {
                    alias: "owner".into(),
                    name: "Owner".into(),
                    role: "owner".into(),
                    sms: Some("+14155550101".into()),
                    sip: None,
                }],
                "create_receipt",
            )
            .unwrap();
        store
            .enqueue_conference_message(
                "ten_local",
                "conv_receipt",
                &members[0],
                &members,
                "msg_receipt",
                "Sandbox update",
                "text/plain",
                "sms",
                None,
                "send_receipt",
                "+14155550000",
            )
            .unwrap();
        let delivery = store.claim_conference_delivery().unwrap().unwrap();
        // Callback wins the race while the provider's submit response is still
        // in flight. Its correlation is not in message_deliveries yet.
        store
            .record_conference_receipt("ten_local", "provider-race", final_state, None)
            .unwrap();
        drop(store);
        let store = Store::open(&cfg).unwrap();
        store
            .update_conference_delivery(
                "ten_local",
                &delivery.id,
                Some("provider-race"),
                "sent",
                None,
            )
            .unwrap();
        let rows = store
            .conference_deliveries("ten_local", "conv_receipt")
            .unwrap();
        assert_eq!(rows[0].state, final_state);
        assert_eq!(rows[0].provider_id.as_deref(), Some("provider-race"));
        store
            .record_conference_receipt("ten_local", "provider-race", "sent", None)
            .unwrap();
        store
            .update_conference_delivery(
                "ten_local",
                &delivery.id,
                Some("provider-race"),
                "sent",
                None,
            )
            .unwrap();
        let events = store
            .conference_events("ten_local", "conv_receipt", &members[0], 0, 500)
            .unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|e| e.event_type == "message.delivery")
                .count(),
            1
        );
        assert_eq!(events.last().unwrap().payload["state"], final_state);
    }
}

#[tokio::test]
async fn retry_is_idempotent_and_content_reuse_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let app = App::new(cfg.clone(), Store::open(&cfg).unwrap()).unwrap();
    let url = url::Url::parse(&format!("ws://{}", app.start_uctp().await.unwrap())).unwrap();
    let mut admin = Peer::connect(&url, "test-admin").await;
    let (cid, members) = provision(&mut admin).await;
    let mut worker = member_peer(&app, &url, &cid, &members[4]).await;
    let request = message(
        &cid,
        "msg_once",
        vec![members[3].participant_id.clone()],
        "Confirm pickup",
    );
    let first = worker.request(request.clone()).await;
    assert_eq!(first.msg_type, MessageType::Ack);
    let mut owner = member_peer(&app, &url, &cid, &members[0]).await;
    let inspected = owner
        .request(command(
            MessageType::Unknown("conversation.inspect".into()),
            Some(&cid),
            json!({"request_id":request.id}),
        ))
        .await;
    assert_eq!(
        inspected.payload["evidence"]["request"],
        serde_json::to_value(&request).unwrap()
    );
    assert_eq!(
        inspected.payload["evidence"]["response"],
        serde_json::to_value(&first).unwrap()
    );
    let mut companion = member_peer(&app, &url, &cid, &members[1]).await;
    assert_eq!(
        companion
            .request(command(
                MessageType::Unknown("conversation.inspect".into()),
                Some(&cid),
                json!({"request_id":request.id})
            ))
            .await
            .payload["code"],
        403
    );
    let second = worker.request(request.clone()).await;
    assert_eq!(second.payload, first.payload);
    let mut changed = request;
    changed.payload["body"] = json!("Different text");
    let rejected = worker.request(changed).await;
    assert_eq!(rejected.payload["code"], 409);
    wait_sent(&app, &cid, 1).await;
    assert_eq!(
        app.state
            .store
            .list_messages("ten_local", &cid)
            .unwrap()
            .len(),
        1
    );
    // Replay from a new physical peer also reads the durable outcome.
    let mut reconnected = member_peer(&app, &url, &cid, &members[4]).await;
    let mut retry = message(
        &cid,
        "msg_once",
        vec![members[3].participant_id.clone()],
        "Confirm pickup",
    );
    retry.id = first.in_reply_to.unwrap();
    assert_eq!(reconnected.request(retry).await.payload, first.payload);
}

#[tokio::test]
async fn membership_visibility_and_two_conversation_isolation() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let app = App::new(cfg.clone(), Store::open(&cfg).unwrap()).unwrap();
    let url = url::Url::parse(&format!("ws://{}", app.start_uctp().await.unwrap())).unwrap();
    let mut admin = Peer::connect(&url, "test-admin").await;
    let (cid, a) = provision(&mut admin).await;
    let (other, b) = provision(&mut admin).await;
    let mut worker = member_peer(&app, &url, &cid, &a[4]).await;
    let mut second = member_peer(&app, &url, &other, &b[4]).await;
    for (peer, cid, recipient, mid) in [
        (&mut worker, &cid, &a[3], "msg_a"),
        (&mut second, &other, &b[3], "msg_b"),
    ] {
        assert_eq!(
            peer.request(message(
                cid,
                mid,
                vec![recipient.participant_id.clone()],
                "Identical text"
            ))
            .await
            .msg_type,
            MessageType::Ack
        );
    }
    assert_eq!(
        worker
            .request(message(
                &other,
                "msg_wrong",
                vec![b[3].participant_id.clone()],
                "Do not send"
            ))
            .await
            .payload["code"],
        403
    );
    let mut companion = member_peer(&app, &url, &cid, &a[1]).await;
    let history = companion
        .request(command(MessageType::MessageHistory, Some(&cid), json!({})))
        .await;
    assert!(history.payload["messages"].as_array().unwrap().is_empty());
    let snapshot = companion
        .request(command(
            MessageType::Unknown("conversation.subscribe".into()),
            Some(&cid),
            json!({"after":0}),
        ))
        .await;
    assert!(snapshot.payload["events"]
        .as_array()
        .unwrap()
        .iter()
        .all(|e| e["event_type"] != "message.accepted"));
    assert!(snapshot.payload["participants"][3]["sms"].is_null());
    wait_sent(&app, &cid, 1).await;
    wait_sent(&app, &other, 1).await;
    assert!(matches!(
        app.state
            .store
            .receive_conference_sms(
                "ten_local",
                "ambiguous_reply",
                "+14155550104",
                "+14155550000",
                "Which pickup?"
            )
            .unwrap(),
        InboundSmsOutcome::Held { .. }
    ));
    assert_eq!(
        app.state
            .store
            .list_messages("ten_local", &cid)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        app.state
            .store
            .list_messages("ten_local", &other)
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn rejected_recipients_are_atomic_and_delivery_states_do_not_regress() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let app = App::new(cfg.clone(), Store::open(&cfg).unwrap()).unwrap();
    let url = url::Url::parse(&format!("ws://{}", app.start_uctp().await.unwrap())).unwrap();
    let mut admin = Peer::connect(&url, "test-admin").await;
    let (cid, m) = provision(&mut admin).await;
    let mut worker = member_peer(&app, &url, &cid, &m[4]).await;
    let bad = worker
        .request(message(
            &cid,
            "msg_bad",
            vec![m[3].participant_id.clone(), "part_unknown".into()],
            "Do not partially send",
        ))
        .await;
    assert_eq!(bad.payload["code"], 403);
    assert!(app
        .state
        .store
        .list_messages("ten_local", &cid)
        .unwrap()
        .is_empty());
    assert_eq!(
        worker
            .request(message(
                &cid,
                "msg_status",
                vec![m[3].participant_id.clone()],
                "Pickup"
            ))
            .await
            .msg_type,
        MessageType::Ack
    );
    wait_sent(&app, &cid, 1).await;
    let d = app
        .state
        .store
        .conference_deliveries("ten_local", &cid)
        .unwrap()
        .remove(0);
    app.state
        .store
        .update_conference_delivery(
            "ten_local",
            &d.id,
            d.provider_id.as_deref(),
            "delivered",
            None,
        )
        .unwrap();
    app.state
        .store
        .update_conference_delivery("ten_local", &d.id, d.provider_id.as_deref(), "sent", None)
        .unwrap();
    app.state
        .store
        .update_conference_delivery(
            "ten_local",
            &d.id,
            d.provider_id.as_deref(),
            "delivered",
            None,
        )
        .unwrap();
    assert_eq!(
        app.state
            .store
            .conference_deliveries("ten_local", &cid)
            .unwrap()[0]
            .state,
        "delivered"
    );
    let events = app
        .state
        .store
        .conference_events("ten_local", &cid, &m[4], 0, 500)
        .unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|e| e.payload["state"] == "delivered")
            .count(),
        1
    );
    assert!(events.iter().all(|e| e.payload["state"] != "confirmed"));
}

#[tokio::test]
async fn external_js_worker_uses_vapi_decisions_and_only_uctp_communications() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let app = App::new(cfg.clone(), Store::open(&cfg).unwrap()).unwrap();
    let url = url::Url::parse(&format!("ws://{}", app.start_uctp().await.unwrap())).unwrap();
    let mut admin = Peer::connect(&url, "test-admin").await;
    let (cid, m) = provision(&mut admin).await;
    let fixture = dir.path().join("worker-fixture.json");
    std::fs::write(&fixture,json!({
        "cid":cid,"url":url.as_str(),"assistant_id":m[4].participant_id,"owner_id":m[0].participant_id,
        "assistant_token":app.state.store.issue_conference_token("ten_local",&cid,&m[4].participant_id).unwrap(),
        "owner_token":app.state.store.issue_conference_token("ten_local",&cid,&m[0].participant_id).unwrap(),
        "state_path":dir.path().join("worker.json")
    }).to_string()).unwrap();
    let output = tokio::time::timeout(
        Duration::from_secs(30),
        tokio::process::Command::new("node")
            .arg("examples/conference-assistant/integration-test.mjs")
            .arg(fixture)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    wait_sent(&app, &cid, 4).await;
    let deliveries = app
        .state
        .store
        .conference_deliveries("ten_local", &cid)
        .unwrap();
    assert_eq!(deliveries.len(), 4);
    for member in &m[..4] {
        assert_eq!(
            deliveries
                .iter()
                .filter(|d| d.participant_id == member.participant_id)
                .count(),
            1
        );
    }
}

#[cfg(feature = "sip")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_sip_invite_replay_and_end_keep_the_same_conversation() {
    use rvoip_sip::{Config as SipConfig, Endpoint, EndpointProfile};
    let a = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let b = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let local = a.local_addr().unwrap();
    let remote = b.local_addr().unwrap();
    drop(a);
    drop(b);
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config(dir.path());
    cfg.conference_sip_bind = Some(local.to_string());
    let app = App::new(cfg.clone(), Store::open(&cfg).unwrap()).unwrap();
    parley::conference_voice::bind(&app.state).await.unwrap();
    let mut peer_config = SipConfig::on("booker", remote.ip(), remote.port());
    peer_config.media_port_start = 45000;
    peer_config.media_port_end = 45200;
    let mut endpoint = Endpoint::builder()
        .name("booker")
        .profile(EndpointProfile::Custom(peer_config))
        .build()
        .await
        .unwrap();
    let (answered_tx, answered_rx) = tokio::sync::oneshot::channel();
    let peer = tokio::spawn(async move {
        let incoming = tokio::time::timeout(Duration::from_secs(15), endpoint.wait_for_incoming())
            .await
            .unwrap()
            .unwrap();
        let call = incoming.answer().await.unwrap();
        let _ = answered_tx.send(());
        tokio::time::timeout(Duration::from_secs(15), call.wait_for_end(None))
            .await
            .unwrap()
            .unwrap();
    });
    let url = url::Url::parse(&format!("ws://{}", app.start_uctp().await.unwrap())).unwrap();
    let mut admin = Peer::connect(&url, "test-admin").await;
    let (cid, m) = provision_with_sip(&mut admin, &format!("sip:booker@{remote}")).await;
    let mut worker = member_peer(&app, &url, &cid, &m[4]).await;
    let invite = command(
        MessageType::SessionInvite,
        Some(&cid),
        json!({"medium":"voice","to":m[2].participant_id,"purpose":"Ask for the sandbox alternate itinerary"}),
    );
    let accepted = worker.request(invite.clone()).await;
    assert_eq!(accepted.msg_type, MessageType::Ack, "{}", accepted.payload);
    assert_eq!(worker.request(invite).await.payload, accepted.payload);
    tokio::time::timeout(Duration::from_secs(15), answered_rx)
        .await
        .unwrap()
        .unwrap();
    let sid = accepted.payload["session"]["sid"].as_str().unwrap();
    let connid = accepted.payload["session"]["connid"].as_str().unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if app
                .state
                .store
                .get_connection("ten_local", connid)
                .unwrap()
                .unwrap()
                .state
                == "connected"
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        app.state
            .store
            .list_sessions("ten_local", &cid)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        app.state
            .orchestrator
            .session_of(&rvoip_core::ids::ConnectionId::from_string(connid))
            .unwrap()
            .to_string(),
        sid
    );
    let mut end = command(MessageType::SessionEnd, Some(&cid), json!({}));
    end.sid = Some(sid.into());
    let ended = worker.request(end).await;
    assert_eq!(ended.msg_type, MessageType::Ack, "{}", ended.payload);
    peer.await.unwrap();
    assert_eq!(
        app.state
            .store
            .get_session("ten_local", sid)
            .unwrap()
            .unwrap()
            .state,
        "ended"
    );
    assert_eq!(
        app.state
            .store
            .get_conversation("ten_local", &cid)
            .unwrap()
            .unwrap()
            .state,
        "open"
    );
    assert_eq!(
        worker
            .request(message(
                &cid,
                "msg_after_voice",
                vec![m[2].participant_id.clone()],
                "Thanks for the itinerary"
            ))
            .await
            .msg_type,
        MessageType::Ack
    );
    wait_sent(&app, &cid, 1).await;
}

#[test]
fn interrupted_submission_is_journaled_once_and_never_reclaimed() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let store = Store::open(&cfg).unwrap();
    let roster: Vec<parley::store::conference::MemberInput> = serde_json::from_value(json!([
        {"alias":"owner","name":"Owner","role":"owner","sms":"+14155550101"},
        {"alias":"organizer","name":"Organizer","role":"organizer","sms":"+14155550104"},
        {"alias":"companion","name":"Companion","role":"companion","sms":"+14155550102"}
    ]))
    .unwrap();
    let members = store
        .create_conference("ten_local", "conv_recovery", &roster, "create_recovery")
        .unwrap();
    for mid in ["msg_interrupted", "msg_queued"] {
        store
            .enqueue_conference_message(
                "ten_local",
                "conv_recovery",
                &members[0],
                &members[1..2],
                mid,
                "Sandbox update",
                "text/plain",
                "sms",
                None,
                mid,
                "+14155550000",
            )
            .unwrap();
    }
    let interrupted = store.claim_conference_delivery().unwrap().unwrap();
    drop(store);
    let store = Store::open(&cfg).unwrap();
    store.recover_conference_outbox().unwrap();
    store.recover_conference_outbox().unwrap();
    let deliveries = store
        .conference_deliveries("ten_local", "conv_recovery")
        .unwrap();
    assert_eq!(
        deliveries
            .iter()
            .find(|d| d.id == interrupted.id)
            .unwrap()
            .state,
        "unknown"
    );
    let queued = store.claim_conference_delivery().unwrap().unwrap();
    assert_ne!(queued.id, interrupted.id);
    assert!(store.claim_conference_delivery().unwrap().is_none());
    for member in &members[..2] {
        let events = store
            .conference_events("ten_local", "conv_recovery", member, 0, 500)
            .unwrap();
        let outcomes: Vec<_> = events
            .iter()
            .filter(|e| e.event_type == "message.delivery")
            .collect();
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].payload["state"], "unknown");
        assert_eq!(
            outcomes[0].request_id.as_deref(),
            Some(interrupted.message_id.as_str())
        );
    }
    let private = store
        .conference_events("ten_local", "conv_recovery", &members[2], 0, 500)
        .unwrap();
    assert!(!private.iter().any(|e| e.event_type == "message.delivery"));
}

#[tokio::test]
async fn restart_voice_requires_owner_verification_over_uctp_and_replays_once() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let store = Store::open(&cfg).unwrap();
    let roster: Vec<parley::store::conference::MemberInput> = serde_json::from_value(json!([
        {"alias":"owner","name":"Owner","role":"owner"},
        {"alias":"organizer","name":"Organizer","role":"organizer"},
        {"alias":"assistant","name":"Assistant","role":"assistant"},
        {"alias":"companion","name":"Companion","role":"companion"}
    ]))
    .unwrap();
    let cid = "conv_restart";
    let members = store
        .create_conference("ten_local", cid, &roster, "create_restart")
        .unwrap();
    store
        .prepare_conference_voice(
            "ten_local",
            cid,
            "ses_restart",
            "invite_restart",
            &members[1],
            &members[2],
            "conn_restart",
            "Sandbox confirmation",
            &members[2].participant_id,
        )
        .unwrap();
    drop(store);
    // Actual startup recovery on a reopened database, with no remote provider.
    let app = App::new(cfg.clone(), Store::open(&cfg).unwrap()).unwrap();
    app.state.store.recover_conference_voice().unwrap();
    assert_eq!(
        app.state
            .store
            .count_live_voice_sessions("ten_local", cid)
            .unwrap(),
        1
    );
    assert_eq!(
        app.state
            .store
            .get_connection("ten_local", "conn_restart")
            .unwrap()
            .unwrap()
            .state,
        "unknown"
    );
    let events = app
        .state
        .store
        .conference_events("ten_local", cid, &members[0], 0, 500)
        .unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|e| e.event_type == "session.interrupted")
            .count(),
        1
    );
    let private = app
        .state
        .store
        .conference_events("ten_local", cid, &members[3], 0, 500)
        .unwrap();
    assert!(!private
        .iter()
        .any(|e| e.event_type == "session.interrupted"));
    let url = url::Url::parse(&format!("ws://{}", app.start_uctp().await.unwrap())).unwrap();
    let mut owner = member_peer(&app, &url, cid, &members[0]).await;
    let mut assistant = member_peer(&app, &url, cid, &members[2]).await;
    let mut confirm = command(
        MessageType::SessionUpdate,
        Some(cid),
        json!({"kind":"confirm_ended","verification_note":"Organizer checked the remote phone and confirmed it disconnected."}),
    );
    confirm.sid = Some("ses_restart".into());
    let rejected = assistant.request(confirm.clone()).await;
    assert_eq!(rejected.msg_type, MessageType::Error);
    assert_eq!(rejected.payload["code"], 403);
    let mut empty = confirm.clone();
    empty.id = "env_empty_note".into();
    empty.payload["verification_note"] = json!(" ");
    assert_eq!(owner.request(empty).await.payload["code"], 400);
    let mut wrong = confirm.clone();
    wrong.id = "env_wrong_session".into();
    wrong.sid = Some("ses_other".into());
    assert_eq!(owner.request(wrong).await.payload["code"], 409);
    let accepted = owner.request(confirm.clone()).await;
    assert_eq!(accepted.msg_type, MessageType::Ack, "{}", accepted.payload);
    assert_eq!(accepted.payload["session"]["source"], "owner_verification");
    assert_eq!(
        accepted.payload["session"]["verified_by"],
        members[0].participant_id
    );
    assert_eq!(owner.request(confirm).await.id, accepted.id);
    assert_eq!(
        app.state
            .store
            .count_live_voice_sessions("ten_local", cid)
            .unwrap(),
        0
    );
    let events = app
        .state
        .store
        .conference_events("ten_local", cid, &members[0], 0, 500)
        .unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|e| e.event_type == "session.ended")
            .count(),
        1
    );
    assert_eq!(
        app.state
            .store
            .get_conversation("ten_local", cid)
            .unwrap()
            .unwrap()
            .state,
        "open"
    );
}

#[test]
fn ai_transport_failure_remains_a_failure_after_teardown_but_retired_ai_cannot_fail_human_voice() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let store = Store::open(&cfg).unwrap();
    let roster: Vec<parley::store::conference::MemberInput> = serde_json::from_value(json!([
        {"alias":"owner","name":"Owner","role":"owner"},
        {"alias":"remote","name":"Remote","role":"organizer"},
        {"alias":"ai","name":"David","role":"assistant"}
    ])).unwrap();
    for (index, retired, ended, intentional) in [
        (0, false, false, false), (1, false, true, false),
        (2, true, false, false), (3, false, true, true),
    ] {
        let cid = format!("conv_ai_failure_{index}");
        let sid = format!("sess_ai_failure_{index}");
        let remote = format!("conn_remote_{index}");
        let ai = format!("conn_ai_{index}");
        let members = store.create_conference("ten_local", &cid, &roster, "create").unwrap();
        store.prepare_conference_voice("ten_local", &cid, &sid, "invite", &members[1], &members[2], &remote, "test", &members[2].participant_id).unwrap();
        let voice = store.conference_voice_for_session("ten_local", &sid).unwrap().unwrap();
        store.conference_ai_attached("ten_local", &voice, &ai, "bridge_ai").unwrap();
        if retired {
            let browser = format!("conn_browser_{index}");
            store.prepare_conference_browser("ten_local", &voice, &members[0].participant_id, &browser, "browser").unwrap();
            store.update_conference_browser("ten_local", &browser, "speaking", Some("handoff"), json!({"bridge_id":"bridge_browser","retained_connid":remote})).unwrap();
        }
        if intentional {
            assert!(!store.mark_conference_ai_ending("ten_other", &sid).unwrap());
            assert!(store.mark_conference_ai_ending("ten_local", &sid).unwrap());
            assert!(!store.mark_conference_ai_ending("ten_local", &sid).unwrap());
        }
        if ended { store.finish_conference_voice("ten_local", &sid, "ended", None).unwrap(); }
        assert!(!store.record_conference_ai_failure("ten_other", &ai).unwrap());
        assert_eq!(store.record_conference_ai_failure("ten_local", &ai).unwrap(), !retired && !intentional);
        assert!(!store.record_conference_ai_failure("ten_local", &ai).unwrap());
        assert!(!store.mark_conference_ai_ending("ten_local", &sid).unwrap());
        let events = store.conference_events("ten_local", &cid, &members[0], 0, 500).unwrap();
        let failures: Vec<_> = events.iter().filter(|event| event.event_type == "session.assistant_failed").collect();
        assert_eq!(failures.len(), usize::from(!retired && !intentional));
        if let Some(failure) = failures.first() {
            assert_eq!(failure.payload["connid"], ai);
            assert_eq!(failure.request_id.as_deref(), Some("invite"));
            assert_eq!(store.get_connection("ten_local", &ai).unwrap().unwrap().state, "failed");
        } else if retired {
            assert_eq!(store.speaking_route("ten_local", &sid).unwrap().unwrap().participant_id, members[0].participant_id);
            assert_eq!(store.get_session("ten_local", &sid).unwrap().unwrap().state, "active");
        } else {
            assert_eq!(store.get_session("ten_local", &sid).unwrap().unwrap().state, "ended");
        }
    }
}

#[tokio::test]
async fn normal_remote_hangup_does_not_hide_ai_failures_that_caused_paired_teardown() {
    use rvoip_core::{events::Event, ConnectionId, EndReason};
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let app = App::new(cfg.clone(), Store::open(&cfg).unwrap()).unwrap();
    let store = &app.state.store;
    let roster: Vec<parley::store::conference::MemberInput> = serde_json::from_value(json!([
        {"alias":"owner","name":"Owner","role":"owner"},
        {"alias":"remote","name":"Remote","role":"organizer"},
        {"alias":"ai","name":"David","role":"assistant"}
    ])).unwrap();
    for (index, reason, failure_first, failed) in [
        (0, EndReason::Normal, false, false),
        (1, EndReason::BridgeTorn, false, true),
        (2, EndReason::Normal, true, true),
    ] {
        let cid = format!("conv_ai_mirror_{index}");
        let sid = format!("sess_ai_mirror_{index}");
        let remote = format!("conn_remote_{index}");
        let ai = format!("conn_ai_{index}");
        let members = store.create_conference("ten_local", &cid, &roster, "create").unwrap();
        store.prepare_conference_voice("ten_local", &cid, &sid, "invite", &members[1], &members[2], &remote, "test", &members[2].participant_id).unwrap();
        let voice = store.conference_voice_for_session("ten_local", &sid).unwrap().unwrap();
        store.conference_ai_attached("ten_local", &voice, &ai, "bridge_ai").unwrap();
        let failure = Event::ConnectionFailed {
            connection_id: ConnectionId::from_string(&ai), detail: "provider socket closed".into(), at: chrono::Utc::now(),
        };
        if failure_first { parley::conference_voice::mirror(&app.state, &failure).unwrap(); }
        parley::conference_voice::mirror(&app.state, &Event::ConnectionEnded {
            connection_id: ConnectionId::from_string(&remote), reason, at: chrono::Utc::now(),
        }).unwrap();
        if !failure_first { parley::conference_voice::mirror(&app.state, &failure).unwrap(); }
        let events = store.conference_events("ten_local", &cid, &members[0], 0, 500).unwrap();
        assert_eq!(events.iter().filter(|event| event.event_type == "session.assistant_failed").count(), usize::from(failed));
    }
}

#[test]
fn phone_move_admission_and_restart_never_redial_or_fabricate_a_commit() {
    let dir=tempfile::tempdir().unwrap();let cfg=config(dir.path());let store=Store::open(&cfg).unwrap();
    let roster:Vec<parley::store::conference::MemberInput>=serde_json::from_value(json!([
        {"alias":"owner","name":"Owner","role":"owner"},
        {"alias":"remote","name":"Remote","role":"organizer"},
        {"alias":"ai","name":"AI","role":"assistant"}
    ])).unwrap();
    let members=store.create_conference("ten_local","conv_phone_recovery",&roster,"create").unwrap();
    store.prepare_conference_voice("ten_local","conv_phone_recovery","sess_phone_recovery","invite",&members[1],&members[2],"conn_remote","test",&members[2].participant_id).unwrap();
    let voice=store.conference_voice_for_session("ten_local","sess_phone_recovery").unwrap().unwrap();
    store.conference_ai_attached("ten_local",&voice,"conn_ai","bridge_ai").unwrap();
    store.prepare_conference_browser("ten_local",&voice,&members[0].participant_id,"conn_browser","browser").unwrap();
    store.update_conference_browser("ten_local","conn_browser","speaking",Some("handoff"),json!({"bridge_id":"bridge_browser","retained_connid":"conn_remote"})).unwrap();
    let route=store.speaking_route("ten_local","sess_phone_recovery").unwrap().unwrap();
    assert_eq!(route.bridge_id,"bridge_browser");assert_eq!(route.participant_id,members[0].participant_id);
    assert!(store.prepare_phone_move("ten_local","conv_other","sess_phone_recovery",&members[0],&route,"conn_foreign","foreign").is_err());
    assert!(store.prepare_phone_move("ten_local","conv_phone_recovery","sess_phone_recovery",&members[2],&route,"conn_ai_phone","ai_phone").is_err());
    store.prepare_phone_move("ten_local","conv_phone_recovery","sess_phone_recovery",&members[0],&route,"conn_phone","move").unwrap();
    assert!(store.prepare_phone_move("ten_local","conv_phone_recovery","sess_phone_recovery",&members[0],&route,"conn_duplicate","duplicate").is_err());
    assert!(store.get_connection("ten_local","conn_duplicate").unwrap().is_none());
    assert!(store.phone_move("ten_other","conn_phone").unwrap().is_none());
    assert!(!store.transition_phone_move("ten_local","conn_phone",&["answered"],"confirmed",json!({})).unwrap());
    store.transition_phone_move("ten_local","conn_phone",&["prepared"],"answered",json!({})).unwrap();
    assert!(store.transition_phone_move("ten_local","conn_phone",&["answered"],"confirmed",json!({})).unwrap());
    assert!(!store.transition_phone_move("ten_local","conn_phone",&["answered"],"confirmed",json!({})).unwrap());
    drop(store);
    let store=Store::open(&cfg).unwrap();store.recover_conference_voice().unwrap();store.recover_conference_voice().unwrap();
    assert_eq!(store.phone_move("ten_local","conn_phone").unwrap().unwrap().state,"interrupted");
    assert_eq!(store.get_connection("ten_local","conn_phone").unwrap().unwrap().state,"unknown");
    assert!(!store.transition_phone_move("ten_local","conn_phone",&["confirmed"],"committing",json!({})).unwrap());
    assert!(store.prepare_phone_move("ten_local","conv_phone_recovery","sess_phone_recovery",&members[0],&route,"conn_redial","redial").is_err());
    let events=store.conference_events("ten_local","conv_phone_recovery",&members[0],0,500).unwrap();
    assert_eq!(events.iter().filter(|e|e.event_type=="phone.prepared").count(),1);
    assert_eq!(events.iter().filter(|e|e.event_type=="phone.confirmed").count(),1);
    assert!(!events.iter().any(|e|e.event_type=="phone.speaking"));
}

#[tokio::test]
async fn signed_sms_keywords_do_not_become_task_replies_and_normal_replies_stay_correlated() {
    use axum::{body::Body, http::Request};
    use base64::{engine::general_purpose::STANDARD, Engine};
    use ed25519_dalek::{Signer, SigningKey};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let mut app = App::new(cfg.clone(), Store::open(&cfg).unwrap()).unwrap();
    // A deterministic local signing key, unrelated to the real carrier account.
    let key = SigningKey::from_bytes(&[42; 32]);
    app.state.telnyx_verifier = Some(Arc::new(
        telnyx::webhooks::Verifier::new(&STANDARD.encode(key.verifying_key().to_bytes())).unwrap(),
    ));
    let url = url::Url::parse(&format!("ws://{}", app.start_uctp().await.unwrap())).unwrap();
    let mut admin = Peer::connect(&url, "test-admin").await;
    let (cid, members) = provision(&mut admin).await;
    let mut assistant = member_peer(&app, &url, &cid, &members[4]).await;
    assert_eq!(
        assistant
            .request(message(
                &cid,
                "msg_keyword_route",
                vec![members[3].participant_id.clone()],
                "Confirm pickup"
            ))
            .await
            .msg_type,
        MessageType::Ack
    );
    wait_sent(&app, &cid, 1).await;

    let callback = |id: &str, text: &str, kind: Value| {
        json!({"data":{
        "id":format!("event_{id}"), "event_type":"message.received", "occurred_at":"2026-10-06T19:00:00Z",
        "payload":{"id":id,"direction":"inbound","from":{"phone_number":"+14155550104"},
        "to":[{"phone_number":"+14155550000"}],"text":text,"autoresponse_type":kind}
    }}).to_string()
    };
    let request = |body: String, signed: bool| {
        let ts = chrono::Utc::now().timestamp().to_string();
        let signature = STANDARD.encode(key.sign(format!("{ts}|{body}").as_bytes()).to_bytes());
        let mut builder = Request::builder()
            .method("POST")
            .uri("/v1/sms/inbound")
            .header("content-type", "application/json");
        if signed {
            builder = builder
                .header("telnyx-timestamp", ts)
                .header("telnyx-signature-ed25519", signature);
        }
        builder.body(Body::from(body)).unwrap()
    };
    let unsigned = app
        .router()
        .oneshot(request(callback("unsigned", "STOP", json!("STOP")), false))
        .await
        .unwrap();
    assert_eq!(unsigned.status(), axum::http::StatusCode::UNAUTHORIZED);
    for (i, (text, kind)) in [
        ("STOP", json!("STOP")),
        ("take me off this list", json!("STOP")),
        ("YES", json!("START")),
        ("HELP", json!("HELP")),
        ("unsubscribe", Value::Null),
        (" stop all ", Value::Null),
        ("start", Value::Null),
        ("UNSTOP", Value::Null),
    ]
    .into_iter()
    .enumerate()
    {
        let body = callback(&format!("keyword_{i}"), text, kind);
        // Retried provider callbacks have no application effects either.
        for _ in 0..2 {
            let response = app
                .router()
                .oneshot(request(body.clone(), true))
                .await
                .unwrap();
            assert!(response.status().is_success());
            let value: Value =
                serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                    .unwrap();
            assert_eq!(value["provider_handled_keyword"], true, "{value}");
        }
    }
    assert!(!app
        .state
        .store
        .conference_events("ten_local", &cid, &members[4], 0, 500)
        .unwrap()
        .iter()
        .any(|event| event.event_type == "message.received"));
    assert!(app
        .state
        .store
        .held_conference_sms("ten_local", 0)
        .unwrap()
        .is_empty());
    // An ordinary YES without a provider keyword classification and a sentence
    // containing "help" are genuine replies, not a text-based approval shortcut.
    for (i, text) in ["YES", "Please help with pickup at terminal C"]
        .into_iter()
        .enumerate()
    {
        let body = callback(&format!("normal_{i}"), text, Value::Null);
        for _ in 0..2 {
            let response = app
                .router()
                .oneshot(request(body.clone(), true))
                .await
                .unwrap();
            assert!(response.status().is_success());
            let value: Value =
                serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                    .unwrap();
            assert_eq!(value["conversation_id"], cid);
        }
    }
    let replies: Vec<_> = app
        .state
        .store
        .conference_events("ten_local", &cid, &members[4], 0, 500)
        .unwrap()
        .into_iter()
        .filter(|event| event.event_type == "message.received")
        .collect();
    assert_eq!(replies.len(), 2);
    assert!(replies
        .iter()
        .all(|event| event.payload["from"] == members[3].participant_id));
    assert_eq!(
        app.state
            .store
            .conference_deliveries("ten_local", &cid)
            .unwrap()
            .len(),
        1,
        "keyword handling and replies must not create extra outbound messages"
    );
}

#[tokio::test]
async fn ambiguous_webhook_is_durable_private_and_resolved_once_through_uctp() {
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let app = App::new(cfg.clone(), Store::open(&cfg).unwrap()).unwrap();
    let url = url::Url::parse(&format!("ws://{}", app.start_uctp().await.unwrap())).unwrap();
    let mut admin = Peer::connect(&url, "test-admin").await;
    let (cid, a) = provision(&mut admin).await;
    let (other, b) = provision(&mut admin).await;
    let mut worker = member_peer(&app, &url, &cid, &a[4]).await;
    let mut second = member_peer(&app, &url, &other, &b[4]).await;
    for (peer, conv, pid, mid) in [
        (&mut worker, &cid, &a[3].participant_id, "msg_hold_a"),
        (&mut second, &other, &b[3].participant_id, "msg_hold_b"),
    ] {
        assert_eq!(
            peer.request(message(conv, mid, vec![pid.clone()], "Confirm pickup"))
                .await
                .msg_type,
            MessageType::Ack
        );
    }
    wait_sent(&app, &cid, 1).await;
    wait_sent(&app, &other, 1).await;
    let callback = json!({"data":{"id":"event_held","event_type":"message.received","occurred_at":"2026-10-05T20:00:00Z","payload":{
        "id":"provider_held","direction":"inbound","from":{"phone_number":"+14155550104"},"to":[{"phone_number":"+14155550000"}],"text":"Private pickup instructions"
    }}});
    let request = || {
        Request::builder()
            .method("POST")
            .uri("/v1/sms/inbound")
            .header("content-type", "application/json")
            .body(Body::from(callback.to_string()))
            .unwrap()
    };
    let response = app.router().oneshot(request()).await.unwrap();
    assert!(response.status().is_success());
    let value: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(value["state"], "held", "{value}");
    let id = value["inbox_id"].as_i64().unwrap();
    // A reopened database sees the held text. Closing one candidate never makes
    // a later callback guess that the remaining candidate was intended.
    let reopened = Store::open(&cfg).unwrap();
    assert_eq!(
        reopened.held_conference_sms("ten_local", 0).unwrap()[0].id,
        id
    );
    assert!(reopened
        .held_conference_sms("ten_other", 0)
        .unwrap()
        .is_empty());
    reopened.close_conversation("ten_local", &other).unwrap();
    let response = app.router().oneshot(request()).await.unwrap();
    assert!(response.status().is_success());
    let value: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(value["inbox_id"], id);
    assert_eq!(
        reopened.held_conference_sms("ten_local", 0).unwrap().len(),
        1
    );
    assert!(reopened
        .receive_conference_sms(
            "ten_local",
            "provider_held",
            "+14155550104",
            "+14155550000",
            "Changed payload"
        )
        .is_err());
    let list = || command(MessageType::Unknown("inbox.list".into()), None, json!({}));
    assert_eq!(worker.request(list()).await.payload["code"], 403);
    let held = admin.request(list()).await;
    assert_eq!(
        held.payload["entries"][0]["body"],
        "Private pickup instructions"
    );
    assert_eq!(
        held.payload["entries"][0]["candidates"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let listed = tokio::process::Command::new("node")
        .args(["scripts/conference-inbox.mjs", "list"])
        .env("PARLEY_API_SECRET", "test-admin")
        .env("UCTP_URL", url.as_str())
        .kill_on_drop(true)
        .output()
        .await
        .unwrap();
    assert!(
        listed.status.success(),
        "{}",
        String::from_utf8_lossy(&listed.stderr)
    );
    let listed: Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(listed["id"], id);
    let resolve = |conv: &str, pid: &str, note: &str| {
        command(
            MessageType::Unknown("inbox.resolve".into()),
            None,
            json!({"inbox_id":id,"conversation_id":conv,"participant_id":pid,"verification_note":note}),
        )
    };
    assert_eq!(
        worker
            .request(resolve(&cid, &a[3].participant_id, "Confirmed with sender"))
            .await
            .payload["code"],
        403
    );
    assert_eq!(
        admin
            .request(resolve(&cid, &a[1].participant_id, "Wrong participant"))
            .await
            .payload["code"],
        400
    );
    assert_eq!(
        admin
            .request(resolve(&other, &b[3].participant_id, "Closed Conversation"))
            .await
            .payload["code"],
        409
    );
    assert_eq!(
        admin
            .request(resolve(&cid, &a[3].participant_id, " "))
            .await
            .payload["code"],
        400
    );
    for conv in [&cid, &other] {
        assert_eq!(reopened.list_messages("ten_local", conv).unwrap().len(), 1);
    }
    let resolved_request = resolve(
        &cid,
        &a[3].participant_id,
        "Organizer confirmed this belongs to Jonathan's current trip.",
    );
    let result = admin.request(resolved_request.clone()).await;
    assert_eq!(result.msg_type, MessageType::Ack, "{}", result.payload);
    assert_eq!(result.payload["message"]["conversation_id"], cid);
    assert_eq!(
        result.payload["message"]["from_participant"],
        a[3].participant_id
    );
    assert_eq!(admin.request(resolved_request.clone()).await.id, result.id);
    let file = dir.path().join("resolution.json");
    let mut instruction = resolved_request.payload.clone();
    instruction["request_id"] = json!(resolved_request.id);
    std::fs::write(&file, instruction.to_string()).unwrap();
    let replay = tokio::process::Command::new("node")
        .args(["scripts/conference-inbox.mjs", "resolve"])
        .arg(&file)
        .env("PARLEY_API_SECRET", "test-admin")
        .env("UCTP_URL", url.as_str())
        .kill_on_drop(true)
        .output()
        .await
        .unwrap();
    assert!(
        replay.status.success(),
        "{}",
        String::from_utf8_lossy(&replay.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&replay.stdout).unwrap(),
        result.payload
    );

    assert!(admin.request(list()).await.payload["entries"]
        .as_array()
        .unwrap()
        .is_empty());
    let response = app.router().oneshot(request()).await.unwrap();
    let value: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(value["message_id"], result.payload["message"]["id"]);
    let events = reopened
        .conference_events("ten_local", &cid, &a[4], 0, 500)
        .unwrap();
    let facts: Vec<_> = events
        .iter()
        .filter(|e| e.event_type == "message.received")
        .collect();
    assert_eq!(facts.len(), 1);
    assert_eq!(
        facts[0].request_id.as_deref(),
        Some(resolved_request.id.as_str())
    );
    assert_eq!(
        facts[0].payload["routing"]["source"],
        "administrator_resolution"
    );
    assert_eq!(
        reopened.list_messages("ten_local", &other).unwrap().len(),
        1
    );
    assert!(reopened
        .conference_history("ten_local", &cid, &a[1])
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn history_pages_filter_recipients_before_limits_and_live_leases_are_explicit() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let app = App::new(cfg.clone(), Store::open(&cfg).unwrap()).unwrap();
    let url = url::Url::parse(&format!("ws://{}", app.start_uctp().await.unwrap())).unwrap();
    let mut admin = Peer::connect(&url, "test-admin").await;
    let (cid, members) = provision(&mut admin).await;
    for i in 0..503 {
        let target = if i == 0 || i == 502 { 1 } else { 0 };
        app.state
            .store
            .enqueue_conference_message(
                "ten_local",
                &cid,
                &members[4],
                &members[target..target + 1],
                &format!("msg_page_{i}"),
                "Private task detail",
                "text/plain",
                "chat",
                None,
                &format!("env_page_{i}"),
                "",
            )
            .unwrap();
    }
    let mut worker = member_peer(&app, &url, &cid, &members[4]).await;
    let mut companion = member_peer(&app, &url, &cid, &members[1]).await;
    let first = worker
        .request(command(
            MessageType::MessageHistory,
            Some(&cid),
            json!({"after":0}),
        ))
        .await;
    assert_eq!(first.payload["messages"].as_array().unwrap().len(), 500);
    assert_eq!(first.payload["has_more"], true);
    let second = worker
        .request(command(
            MessageType::MessageHistory,
            Some(&cid),
            json!({"after":first.payload["cursor"]}),
        ))
        .await;
    assert_eq!(second.payload["messages"].as_array().unwrap().len(), 3);
    assert_eq!(second.payload["messages"][0]["id"], "msg_page_500");
    assert_eq!(second.payload["has_more"], false);
    let private = companion
        .request(command(MessageType::MessageHistory, Some(&cid), json!({})))
        .await;
    assert_eq!(private.payload["messages"].as_array().unwrap().len(), 2);
    assert_eq!(private.payload["messages"][1]["id"], "msg_page_502");
    assert_eq!(private.payload["has_more"], false);
    for after in [json!(-1), json!("500")] {
        assert_eq!(
            worker
                .request(command(
                    MessageType::MessageHistory,
                    Some(&cid),
                    json!({"after":after})
                ))
                .await
                .payload["code"],
            400
        );
    }
    let live = worker
        .request(command(
            MessageType::Unknown("conversation.subscribe".into()),
            Some(&cid),
            json!({"live":true}),
        ))
        .await;
    assert_eq!(live.payload["events"].as_array().unwrap().len(), 500);
    assert!(live.payload["subscription"]["id"]
        .as_str()
        .unwrap()
        .starts_with("sub_"));
    assert!(chrono::DateTime::parse_from_rfc3339(
        live.payload["subscription"]["expires_at"].as_str().unwrap()
    )
    .is_ok());
    let duplicate = worker
        .request(command(
            MessageType::Unknown("conversation.subscribe".into()),
            Some(&cid),
            json!({"live":true}),
        ))
        .await;
    assert_eq!(duplicate.payload["code"], 409);
    // A bounded snapshot still works while the live observer exists.
    let rest = worker
        .request(command(
            MessageType::Unknown("conversation.subscribe".into()),
            Some(&cid),
            json!({"after":live.payload["cursor"]}),
        ))
        .await;
    assert!(rest.payload["subscription"].is_null());
    assert!(!rest.payload["events"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn rehearsal_close_is_scoped_replayable_and_retires_reply_routes() {
    use tower::ServiceExt;
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let app = App::new(cfg.clone(), Store::open(&cfg).unwrap()).unwrap();
    let url = url::Url::parse(&format!("ws://{}", app.start_uctp().await.unwrap())).unwrap();
    let mut admin = Peer::connect(&url, "test-admin").await;
    let (cid, members) = provision(&mut admin).await;
    let mut owner = member_peer(&app, &url, &cid, &members[0]).await;
    let mut worker = member_peer(&app, &url, &cid, &members[4]).await;
    let (next, next_members) = provision(&mut admin).await;
    let mut next_owner = member_peer(&app, &url, &next, &next_members[0]).await;
    let readiness = || {
        command(
            MessageType::Unknown("conversation.preflight".into()),
            Some(&next),
            json!({}),
        )
    };
    let report = next_owner.request(readiness()).await;
    assert_eq!(report.payload["readiness"]["overlapping_conversations"], 1);
    assert_eq!(
        report.payload["capabilities"]["implementation"]["rvoip_baseline"],
        "0.3.12"
    );
    assert_eq!(
        report.payload["capabilities"]["implementation"]["rvoip_patched"],
        true
    );
    assert_eq!(
        report.payload["capabilities"]["implementation"]["control_transport"],
        "websocket"
    );

    assert_eq!(report.payload["readiness"]["ready_for_new_task"], false);
    assert!(!report.payload.to_string().contains(&cid));
    let widget =
        parley::auth::mint_widget_token(&cfg, Some("unrelated-visitor".into()), None).unwrap();
    let response = app
        .router()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri(format!("/v1/conversations/{cid}/close"))
                .header("authorization", format!("Bearer {widget}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::FORBIDDEN);

    let close = command(
        MessageType::ConversationClose,
        Some(&cid),
        json!({"verification_note":"Rehearsal ended; stand-ins informed."}),
    );
    assert_eq!(worker.request(close.clone()).await.payload["code"], 403);
    assert_eq!(next_owner.request(close.clone()).await.payload["code"], 403);
    assert_eq!(
        owner
            .request(command(
                MessageType::ConversationClose,
                Some(&cid),
                json!({"verification_note":" "})
            ))
            .await
            .payload["code"],
        400
    );

    // No await between queueing/claiming/recovery and checks: exercise the
    // transaction boundary without letting the background fake outbox race it.
    app.state
        .store
        .enqueue_conference_message(
            "ten_local",
            &cid,
            &members[4],
            &members[3..4],
            "msg_close_pending",
            "Confirm pickup",
            "text/plain",
            "sms",
            None,
            "env_close_pending",
            &cfg.telnyx_from,
        )
        .unwrap();
    assert!(app
        .state
        .store
        .close_conference(
            "ten_local",
            &cid,
            &members[0].participant_id,
            "done",
            "env_queued_close"
        )
        .is_err());
    let delivery = app
        .state
        .store
        .claim_conference_delivery()
        .unwrap()
        .unwrap();
    assert!(app
        .state
        .store
        .close_conference(
            "ten_local",
            &cid,
            &members[0].participant_id,
            "done",
            "env_submitting_close"
        )
        .is_err());
    app.state.store.recover_conference_outbox().unwrap();
    let response = app
        .router()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri(format!("/v1/conversations/{cid}/close"))
                .header("authorization", "Bearer test-admin")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::CONFLICT);
    assert_eq!(owner.request(close.clone()).await.payload["code"], 409);
    app.state
        .store
        .update_conference_delivery(
            "ten_local",
            &delivery.id,
            Some("provider_close_test"),
            "sent",
            None,
        )
        .unwrap();

    app.state
        .store
        .prepare_conference_voice(
            "ten_local",
            &cid,
            "sess_close_test",
            "env_close_voice",
            &members[3],
            &members[4],
            "conn_close_test",
            "test",
            &members[4].participant_id,
        )
        .unwrap();
    let blocked = owner
        .request(command(
            MessageType::ConversationClose,
            Some(&cid),
            json!({"verification_note":"still active"}),
        ))
        .await;
    assert_eq!(blocked.payload["code"], 409);
    app.state.store.recover_conference_voice().unwrap();
    assert!(app
        .state
        .store
        .close_conference(
            "ten_local",
            &cid,
            &members[0].participant_id,
            "interrupted",
            "env_interrupted_close"
        )
        .is_err());
    app.state
        .store
        .confirm_interrupted_voice_ended(
            "ten_local",
            &cid,
            "sess_close_test",
            &members[0],
            "Remote stand-in confirms ended",
            "env_verify_close",
        )
        .unwrap();
    // A rejected command remains rejected on exact replay; a new decision gets a new ID.
    assert_eq!(owner.request(close).await.payload["code"], 409);
    let close = command(
        MessageType::ConversationClose,
        Some(&cid),
        json!({"verification_note":"All calls ended; stand-ins informed."}),
    );
    let result = owner.request(close.clone()).await;
    assert_eq!(
        result.msg_type,
        MessageType::ConversationClosed,
        "{}",
        result.payload
    );
    assert_eq!(owner.request(close.clone()).await.id, result.id);
    assert_eq!(
        next_owner.request(readiness()).await.payload["readiness"]["ready_for_new_task"],
        true
    );
    assert_eq!(
        worker
            .request(message(
                &cid,
                "msg_after_close",
                vec![members[3].participant_id.clone()],
                "Must not send"
            ))
            .await
            .payload["code"],
        409
    );
    let snapshot = owner
        .request(command(
            MessageType::Unknown("conversation.subscribe".into()),
            Some(&cid),
            json!({}),
        ))
        .await;
    assert_eq!(snapshot.payload["state"], "closed");
    assert_eq!(
        snapshot.payload["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["event_type"] == "conversation.closed")
            .count(),
        1
    );
    assert_eq!(
        app.state
            .store
            .conference_deliveries("ten_local", &cid)
            .unwrap()
            .len(),
        1
    );
    let reply = app
        .state
        .store
        .receive_conference_sms(
            "ten_local",
            "late_old_reply",
            "+14155550104",
            &cfg.telnyx_from,
            "A late old reply",
        )
        .unwrap();
    assert!(matches!(reply, InboundSmsOutcome::Unmatched));
    let bundle_path = dir.path().join("provisioned.json");
    let token = app
        .state
        .store
        .issue_conference_token("ten_local", &cid, &members[0].participant_id)
        .unwrap();
    std::fs::write(
        &bundle_path,
        json!({"cid":cid,"url":url.as_str(),"participants":[{"role":"owner","token":token}]})
            .to_string(),
    )
    .unwrap();
    let decision_path = dir.path().join("close.json");
    std::fs::write(
        &decision_path,
        json!({"request_id":close.id,"verification_note":close.payload["verification_note"]})
            .to_string(),
    )
    .unwrap();
    let reset = tokio::process::Command::new("node")
        .arg("scripts/reset-conference.mjs")
        .arg(&bundle_path)
        .arg(&decision_path)
        .kill_on_drop(true)
        .output()
        .await
        .unwrap();
    assert!(
        reset.status.success(),
        "{}",
        String::from_utf8_lossy(&reset.stderr)
    );
    assert!(!String::from_utf8_lossy(&reset.stdout).contains(&token));
    let preflight = tokio::process::Command::new("node")
        .arg("scripts/preflight-conference.mjs")
        .arg(&bundle_path)
        .kill_on_drop(true)
        .output()
        .await
        .unwrap();
    assert_eq!(preflight.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&preflight.stdout).unwrap();
    assert_eq!(report["readiness"]["state"], "closed");
    assert!(!String::from_utf8_lossy(&preflight.stdout).contains(&token));
    assert!(!String::from_utf8_lossy(&preflight.stdout).contains("+1415555"));
    let reopened = Store::open(&cfg).unwrap();
    assert_eq!(
        reopened.conference_preflight("ten_local", &cid).unwrap()["state"],
        "closed"
    );
}
