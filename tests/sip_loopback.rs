use parley::config::Config;
use parley::sip::{admit_invite, Invite};
use parley::store::Store;
use parley::App;

fn app() -> (App, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut cfg = Config::default();
    cfg.sqlite_path = dir.path().join("parley.sqlite").display().to_string();
    cfg.blob_dir = dir.path().join("blobs").display().to_string();
    let store = Store::open(&cfg).expect("store");
    (App::new(cfg, store).expect("app"), dir)
}

#[tokio::test]
async fn sip_loopback_invite_continues_e164_and_busy_on_second_voice() {
    let (app, _dir) = app();
    let first = admit_invite(
        &app.state,
        Invite {
            cli_e164: "+14155550111".into(),
            did: Some("+14155550999".into()),
        },
    )
    .await
    .expect("first invite");
    let parley::sip::Admit::Accepted {
        conversation_id,
        session_id,
    } = first
    else {
        panic!("expected accepted {first:?}");
    };
    assert!(conversation_id.starts_with("conv_"));
    assert!(session_id.starts_with("sess_"));

    let second = admit_invite(
        &app.state,
        Invite {
            cli_e164: "+14155550111".into(),
            did: Some("+14155550999".into()),
        },
    )
    .await
    .expect("second invite");
    match second {
        parley::sip::Admit::Busy {
            conversation_id: cid,
        } => {
            assert_eq!(cid, conversation_id);
        }
        other => panic!("expected busy, got {other:?}"),
    }
}

#[tokio::test]
async fn sip_invite_outside_hours_leaves_voicemail_message() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut cfg = Config::default();
    cfg.sqlite_path = dir.path().join("parley.sqlite").display().to_string();
    cfg.blob_dir = dir.path().join("blobs").display().to_string();
    cfg.hours.windows = vec![parley::config::HoursWindow {
        days: vec![],
        start: "00:00".into(),
        end: "00:00".into(),
    }];
    let store = Store::open(&cfg).expect("store");
    let app = App::new(cfg, store).expect("app");
    let admitted = admit_invite(
        &app.state,
        Invite {
            cli_e164: "+14155550111".into(),
            did: None,
        },
    )
    .await
    .expect("voicemail");
    let parley::sip::Admit::Voicemail {
        conversation_id,
        message_id,
    } = admitted
    else {
        panic!("expected voicemail {admitted:?}");
    };
    let messages = app
        .state
        .store
        .list_messages("ten_local", &conversation_id)
        .unwrap();
    assert!(messages
        .iter()
        .any(|m| m.id == message_id && m.medium == "audio"));
    assert_eq!(
        app.state
            .store
            .count_live_voice_sessions("ten_local", &conversation_id)
            .unwrap(),
        0
    );
}

#[cfg(feature = "sip")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sip_ua_invite_creates_conversation_by_cli() {
    use std::time::Duration;

    use parley::sip;
    use parley::vapi_voice;
    use rvoip_sip::{Config as SipConfig, Endpoint, EndpointProfile};

    fn two_udp_ports() -> (u16, u16) {
        let a = std::net::UdpSocket::bind("127.0.0.1:0").expect("udp a");
        let b = std::net::UdpSocket::bind("127.0.0.1:0").expect("udp b");
        (
            a.local_addr().expect("addr a").port(),
            b.local_addr().expect("addr b").port(),
        )
    }

    let (parley_port, alice_port) = two_udp_ports();
    let dir = tempfile::tempdir().expect("tempdir");
    let mut cfg = Config::default();
    cfg.sqlite_path = dir.path().join("parley.sqlite").display().to_string();
    cfg.blob_dir = dir.path().join("blobs").display().to_string();
    cfg.bind_sip = format!("127.0.0.1:{parley_port}");
    let store = Store::open(&cfg).expect("store");
    let app = App::new(cfg, store).expect("app");
    sip::bind(&app.state).await.expect("sip bind");
    assert_eq!(app.sip_port(), Some(parley_port));
    tokio::time::sleep(Duration::from_millis(500)).await;

    let mut alice_cfg = SipConfig::local("14155550111", alice_port);
    alice_cfg.media_port_start = 43000;
    alice_cfg.media_port_end = 43100;
    let alice = Endpoint::builder()
        .name("14155550111")
        .profile(EndpointProfile::Custom(alice_cfg))
        .from_uri(format!("sip:+14155550111@127.0.0.1:{alice_port}"))
        .build()
        .await
        .expect("alice endpoint");
    let target = format!("sip:parley@127.0.0.1:{parley_port}");
    let call_task = tokio::spawn(async move {
        match alice
            .call_and_wait(&target, Some(Duration::from_secs(12)))
            .await
        {
            Ok(call) => {
                tokio::time::sleep(Duration::from_millis(400)).await;
                let _ = call.hangup_and_wait(Some(Duration::from_secs(5))).await;
                let _ = alice.shutdown().await;
                true
            }
            Err(_) => {
                let _ = alice.shutdown().await;
                false
            }
        }
    });

    let cid = {
        let mut found = None;
        for _ in 0..150 {
            if let Ok(Some(hit)) =
                app.state
                    .store
                    .lookup_identity("ten_local", "e164", "+14155550111")
            {
                found = Some(hit.conversation_id);
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        found.expect("INVITE should create a Conversation keyed by CLI")
    };
    let conv = app
        .state
        .store
        .get_conversation("ten_local", &cid)
        .expect("get")
        .expect("conversation row");
    assert_eq!(conv.state, "open");
    let ids = vapi_voice::ai_and_customer(&app.state, "ten_local", &cid).expect("distinct ids");
    assert_ne!(ids.ai_participant_id, ids.customer_participant_id);

    let _answered = call_task.await.expect("join alice");
    tokio::time::sleep(Duration::from_millis(400)).await;
    let still = app
        .state
        .store
        .get_conversation("ten_local", &cid)
        .expect("get after bye")
        .expect("persisted");
    assert_eq!(still.state, "open", "BYE must persist the Conversation");
}

#[cfg(feature = "sip")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sip_endpoint_loopback_sanity() {
    use std::time::Duration;

    use rvoip_sip::{Config as SipConfig, Endpoint, EndpointProfile};

    let a = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let b = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let bob_port = a.local_addr().unwrap().port();
    let alice_port = b.local_addr().unwrap().port();
    drop((a, b));

    let bob_task = tokio::spawn(async move {
        let mut bob_cfg = SipConfig::local("parley", bob_port);
        bob_cfg.media_port_start = 44000;
        bob_cfg.media_port_end = 44100;
        let mut bob = Endpoint::builder()
            .name("parley")
            .profile(EndpointProfile::Custom(bob_cfg))
            .build()
            .await
            .expect("bob");
        let incoming = bob.wait_for_incoming().await.expect("incoming");
        let call = incoming.answer().await.expect("answer");
        call.wait_for_end(Some(Duration::from_secs(8)))
            .await
            .expect("end");
        bob.shutdown().await
    });

    tokio::time::sleep(Duration::from_millis(400)).await;
    let mut alice_cfg = SipConfig::local("14155550111", alice_port);
    alice_cfg.media_port_start = 45000;
    alice_cfg.media_port_end = 45100;
    let alice = Endpoint::builder()
        .name("14155550111")
        .profile(EndpointProfile::Custom(alice_cfg))
        .from_uri(format!("sip:+14155550111@127.0.0.1:{alice_port}"))
        .build()
        .await
        .expect("alice");
    let target = format!("sip:parley@127.0.0.1:{bob_port}");
    let call = alice
        .call_and_wait(&target, Some(Duration::from_secs(12)))
        .await
        .expect("call");
    tokio::time::sleep(Duration::from_millis(200)).await;
    call.hangup_and_wait(Some(Duration::from_secs(5)))
        .await
        .expect("hangup");
    alice.shutdown().await.expect("alice shutdown");
    bob_task.await.expect("bob join").expect("bob shutdown");
}
