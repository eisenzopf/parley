use parley::config::Config;
use parley::conversation::{self, PostSession};
use parley::pickup::{self, AcceptPickup, PickupState};
use parley::store::Store;
use parley::App;

#[tokio::test]
async fn pickup_accept_promotes_human_and_observes_ai() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::default();
    cfg.sqlite_path = dir.path().join("parley.sqlite").display().to_string();
    cfg.blob_dir = dir.path().join("blobs").display().to_string();
    let store = Store::open(&cfg).unwrap();
    let app = App::new(cfg, store).unwrap();
    let created = conversation::create_or_continue(
        &app.state,
        "ten_local",
        conversation::CreateConversation {
            identity: parley::identity::IngressKeys {
                e164: Some("+14155550111".into()),
                visitor_id: None,
                cookie: None,
            },
            policy: "persistent".into(),
            participants: Vec::new(),
        },
    )
    .await
    .unwrap();
    conversation::start_session(
        &app.state,
        "ten_local",
        &created.id,
        PostSession {
            medium: "text".into(),
            direction: Some("inbound".into()),
        },
    )
    .await
    .unwrap();
    let voice = conversation::start_session(
        &app.state,
        "ten_local",
        &created.id,
        PostSession {
            medium: "voice".into(),
            direction: Some("inbound".into()),
        },
    )
    .await
    .unwrap();
    let events = app
        .state
        .store
        .list_events("ten_local", &created.id)
        .unwrap();
    assert!(events.iter().any(|e| e.event_type == "recording.consented"));
    let texts = app
        .state
        .store
        .list_sessions("ten_local", &created.id)
        .unwrap();
    assert!(texts
        .iter()
        .any(|s| s.medium == "text" && s.state == "ended"));
    pickup::request(&app.state, "ten_local", &created.id, Some(&voice.id))
        .await
        .unwrap();
    pickup::accept(
        &app.state,
        "ten_local",
        &created.id,
        AcceptPickup {
            session_id: Some(voice.id.clone()),
            operator_participant_id: None,
        },
    )
    .await
    .unwrap();
    let parts = app
        .state
        .store
        .list_participants("ten_local", &created.id)
        .unwrap();
    assert!(parts.iter().any(|p| p.kind == "ai" && p.role == "observer"));
    assert!(parts.iter().any(|p| p.kind == "human" && p.role == "agent"));
    let before = app
        .state
        .store
        .list_connections("ten_local", &voice.id)
        .unwrap();
    let customer = before
        .iter()
        .find(|c| c.id.starts_with("conn_customer_"))
        .unwrap()
        .id
        .clone();
    pickup::return_to_ai(&app.state, "ten_local", &created.id)
        .await
        .unwrap();
    let after = app
        .state
        .store
        .list_connections("ten_local", &voice.id)
        .unwrap();
    assert_eq!(
        after
            .iter()
            .find(|c| c.id.starts_with("conn_customer_"))
            .unwrap()
            .id,
        customer
    );
    let _ = PickupState::Accepted;
}
