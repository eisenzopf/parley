//! End-to-end local media gate: real Chrome, SIP/RTP, and a Vapi wire fixture.
use super::*;
use futures_util::{SinkExt, StreamExt};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

pub(super) fn tone(hz: f64, rate: u32, offset: u32) -> Vec<i16> {
    (0..rate / 50)
        .map(|n| {
            (7000.0 * (std::f64::consts::TAU * hz * (offset + n) as f64 / rate as f64).sin()) as i16
        })
        .collect()
}

pub(super) fn frequency(samples: &[i16], rate: u32) -> f64 {
    let crossings = samples.windows(2).filter(|p| p[0] <= 0 && p[1] > 0).count();
    crossings as f64 * rate as f64 / samples.len().max(1) as f64
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
#[ignore = "requires npm ci and an installed Playwright browser; run the explicit conference media gate"]
async fn chrome_sip_handoff_preserves_call_and_bidirectional_audio() {
    use rvoip_sip::{Config as SipConfig, Endpoint, EndpointAudioFrame, EndpointProfile};
    use tokio_tungstenite::tungstenite::Message;
    let _ = tracing_subscriber::fmt()
        .with_env_filter("parley=debug,rvoip_core=info,rvoip_vapi=info")
        .try_init();
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config(dir.path());
    cfg.vapi_assistant_id = "fixture-assistant".into();
    let signaling = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    cfg.conference_sip_bind = Some(signaling.local_addr().unwrap().to_string());
    drop(signaling);

    let ws_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ws_url = format!("ws://{}/audio", ws_listener.local_addr().unwrap());
    let vapi_received = Arc::new(AtomicUsize::new(0));
    let received = vapi_received.clone();
    let provider = tokio::spawn(async move {
        let (tcp, _) = ws_listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
        let mut tick = tokio::time::interval(Duration::from_millis(20));
        let mut position = 0;
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    let pcm: Vec<u8> = tone(440.0, 16000, position).into_iter().flat_map(i16::to_le_bytes).collect();
                    position += 320;
                    if socket.send(Message::Binary(pcm.into())).await.is_err() { break; }
                    if position == 8000 || position == 16000 {
                        let kind = if position == 8000 {"partial"} else {"final"};
                        let speech = json!({"type":"speech-update","role":"user","status":if position==8000 {"started"} else {"stopped"},"turn":1});
                        if socket.send(Message::Text(speech.to_string().into())).await.is_err() {break;}
                        let transcript = json!({"type":"transcript","role":"user","transcriptType":kind,"transcript":"Sandbox pickup confirmed at terminal C."});
                        if socket.send(Message::Text(transcript.to_string().into())).await.is_err() {break;}
                    }
                },
                frame = socket.next() => match frame {
                    Some(Ok(Message::Binary(bytes))) => {
                        let samples: Vec<i16> = bytes.chunks_exact(2).map(|b|i16::from_le_bytes([b[0],b[1]])).collect();
                        if samples.iter().any(|s| s.abs() > 500) && (frequency(&samples,16000)-660.0).abs() < 100.0 {
                            received.fetch_add(1, Ordering::SeqCst);
                        }
                    },
                    Some(Ok(Message::Text(text))) if text.contains("end-call") => {
                        let _ = socket.send(Message::Text(r#"{"type":"status-update","status":"ended"}"#.into())).await;
                        break;
                    },
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _=>{}
                }
            }
        }
    });
    let http_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let api_url =
        url::Url::parse(&format!("http://{}/", http_listener.local_addr().unwrap())).unwrap();
    let call_count = Arc::new(AtomicUsize::new(0));
    let count = call_count.clone();
    let api = tokio::spawn(async move {
        let router = axum::Router::new().route(
            "/call",
            axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
                count.fetch_add(1, Ordering::SeqCst);
                let ws_url = ws_url.clone();
                async move {
                    assert_eq!(body["assistantId"], "fixture-assistant");
                    axum::Json(json!({"id":"fixture-call","transport":{"websocketCallUrl":ws_url}}))
                }
            }),
        );
        axum::serve(http_listener, router).await.unwrap();
    });
    let adapter = rvoip_vapi::VapiAdapter::new(
        rvoip_vapi::VapiConfig::new(rvoip_vapi::VapiApiKey::new("fixture-private-key").unwrap())
            .with_api_base(api_url)
            .with_loopback_test_transport(),
    )
    .unwrap();
    let app = App::with_voice_adapter(cfg.clone(), Store::open(&cfg).unwrap(), adapter).unwrap();
    parley::conference_voice::bind(&app.state).await.unwrap();

    let remote_socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let remote_addr = remote_socket.local_addr().unwrap();
    drop(remote_socket);
    let mut remote_cfg = SipConfig::on("organizer", remote_addr.ip(), remote_addr.port());
    remote_cfg.media_port_start = 45400;
    remote_cfg.media_port_end = 45600;
    let mut endpoint = Endpoint::builder()
        .name("organizer")
        .profile(EndpointProfile::Custom(remote_cfg))
        .build()
        .await
        .unwrap();
    let ended = Arc::new(AtomicBool::new(false));
    let ended_flag = ended.clone();
    let ai_audio = Arc::new(AtomicUsize::new(0));
    let ai_frames = ai_audio.clone();
    let human_audio = Arc::new(AtomicUsize::new(0));
    let human_frames = human_audio.clone();
    let (hangup_tx, mut hangup_rx) = tokio::sync::oneshot::channel::<()>();
    let remote = tokio::spawn(async move {
        let call = endpoint
            .wait_for_incoming()
            .await
            .unwrap()
            .answer()
            .await
            .unwrap();
        let (send, mut recv) = call.audio().await.unwrap().split();
        let mut tick = tokio::time::interval(Duration::from_millis(20));
        let mut position = 0;
        let mut requested_hangup = false;
        let end = call.wait_for_end(None);
        tokio::pin!(end);
        loop {
            tokio::select! {
                result = &mut end => {result.unwrap(); ended_flag.store(true,Ordering::SeqCst); break;},
                result = &mut hangup_rx, if !requested_hangup => {
                    result.unwrap(); requested_hangup = true; call.hangup().await.unwrap();
                },
                _=tick.tick()=>{
                    if send.send(EndpointAudioFrame::new(tone(660.0,8000,position),8000,1,position)).await.is_err() {break;}
                    position+=160;
                },
                frame=recv.recv()=>if let Some(frame)=frame {
                    if frame.samples.iter().any(|s|s.abs()>500) {
                        let hz=frequency(&frame.samples,frame.sample_rate);
                        if (hz-440.0).abs()<80.0 {ai_frames.fetch_add(1,Ordering::SeqCst);}
                        if (hz-880.0).abs()<80.0 {human_frames.fetch_add(1,Ordering::SeqCst);}
                    }
                } else {break;}
            }
        }
        // Media channels may close before the SIP BYE is observed. Require
        // signaling teardown too, rather than treating RTP closure as hangup.
        if !ended_flag.load(Ordering::SeqCst) {
            end.await.unwrap();
            ended_flag.store(true, Ordering::SeqCst);
        }
    });
    let url = url::Url::parse(&format!("ws://{}", app.start_uctp().await.unwrap())).unwrap();
    let mut admin = Peer::connect(&url, "test-admin").await;
    let mut roster: Value = serde_json::from_str(include_str!(
        "../../examples/conference-assistant/contacts.fixture.json"
    ))
    .unwrap();
    roster[3]["sip"] = json!(format!("sip:organizer@{remote_addr}"));
    let opened = admin
        .request(command(
            MessageType::ConversationCreate,
            None,
            json!({"participants":roster}),
        ))
        .await;
    assert_eq!(opened.msg_type, MessageType::ConversationOpened);
    let cid = opened.cid.unwrap();
    let members: Vec<Member> =
        serde_json::from_value(opened.payload["participants"].clone()).unwrap();
    let mut assistant = member_peer(&app, &url, &cid, &members[4]).await;
    let accepted = assistant.request(command(MessageType::SessionInvite,Some(&cid),
        json!({"medium":"voice","to":members[3].participant_id,"purpose":"Confirm sandbox arrangements"}))).await;
    assert_eq!(accepted.msg_type, MessageType::Ack, "{}", accepted.payload);
    let sid = accepted.payload["session"]["sid"]
        .as_str()
        .unwrap()
        .to_string();
    let connid = accepted.payload["session"]["connid"]
        .as_str()
        .unwrap()
        .to_string();
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if ai_audio.load(Ordering::SeqCst) > 5 && vapi_received.load(Ordering::SeqCst) > 5 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("SIP and Vapi must exchange audible tones in BOTH directions before handoff");
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let snapshot = assistant
                .request(command(
                    MessageType::Unknown("conversation.subscribe".into()),
                    Some(&cid),
                    json!({"after":0}),
                ))
                .await;
            let transcripts: Vec<_> = snapshot.payload["events"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|event| event["event_type"] == "session.transcript")
                .collect();
            if !transcripts.is_empty() {
                let speech:Vec<_>=snapshot.payload["events"].as_array().unwrap().iter().filter(|event|event["event_type"]=="session.speech").collect();
                assert_eq!(speech.len(),2,"speech activity must be published through UCTP");
                assert_eq!(speech[0]["payload"]["state"],"started");
                assert_eq!(speech[1]["payload"]["state"],"stopped");
                for event in speech {
                    assert_eq!(event["payload"]["speaker"],members[3].participant_id);
                    assert_eq!(event["payload"]["sid"],sid);
                    assert_eq!(event["payload"]["source"],"vapi");
                }
                assert_eq!(
                    transcripts.len(),
                    1,
                    "partial transcripts must not become final facts"
                );
                let fact = &transcripts[0]["payload"];
                assert_eq!(fact["speaker"], members[3].participant_id);
                assert_eq!(fact["sid"], sid);
                assert_eq!(fact["source"], "vapi");
                assert_eq!(fact["is_final"], true);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("external UCTP observer must receive the attributed final voice transcript");
    let sip_stream = app
        .state
        .orchestrator
        .wait_for_stream(
            rvoip_core::ids::ConnectionId::from_string(&connid),
            rvoip_core::stream::StreamSelector::new(rvoip_core::stream::StreamKind::Audio)
                .with_readiness(rvoip_core::stream::MediaReadiness::Bidirectional),
            tokio::time::Instant::now() + Duration::from_secs(2),
            Default::default(),
        )
        .await
        .unwrap();
    assert_eq!(sip_stream.codec().clock_rate_hz, 8000);
    assert!(matches!(
        sip_stream.codec().name.to_lowercase().as_str(),
        "pcmu" | "g.711-mu" | "pcma" | "g.711-a"
    ));
    let token = app
        .state
        .store
        .issue_conference_token("ten_local", &cid, &members[0].participant_id)
        .unwrap();
    let web_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let web_url = format!("http://{}", web_listener.local_addr().unwrap());
    let retained = rvoip_core::ids::ConnectionId::from_string(&connid);
    let orchestrator = app.state.orchestrator.clone();
    let original_peer = orchestrator.bridge_peer_of(&retained).unwrap();
    let observed_ai = ai_audio.clone();
    let observed_human = human_audio.clone();
    let observed_provider = vapi_received.clone();
    let router=app.router().route("/__media_fixture",axum::routing::get(move || {
        let result=json!({
            "original_bridge":orchestrator.bridge_peer_of(&retained).as_ref()==Some(&original_peer),
            "ai_frames":observed_ai.load(Ordering::SeqCst),
            "human_frames":observed_human.load(Ordering::SeqCst),
            "provider_frames":observed_provider.load(Ordering::SeqCst),
        });
        async move {axum::Json(result)}
    }));
    let web = tokio::spawn(async move {
        axum::serve(web_listener, router).await.unwrap();
    });
    let output = tokio::time::timeout(
        Duration::from_secs(75),
        tokio::process::Command::new("node")
            .arg("e2e/conference-media.mjs")
            .env(
                "MEDIA_FIXTURE",
                json!({"http":web_url,"url":url.as_str(),"cid":cid,"sid":sid,"token":token})
                    .to_string(),
            )
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        output.status.success(),
        "browser media fixture failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    println!("{}", String::from_utf8_lossy(&output.stdout));
    assert!(
        human_audio.load(Ordering::SeqCst) > 5,
        "SIP must receive the browser's 880 Hz tone after handoff"
    );
    assert!(
        !ended.load(Ordering::SeqCst),
        "original SIP call ended during handoff"
    );
    assert_eq!(
        call_count.load(Ordering::SeqCst),
        1,
        "Vapi call must not be restarted"
    );
    let remote_id = rvoip_core::ids::ConnectionId::from_string(&connid);
    assert_eq!(
        app.state
            .orchestrator
            .session_of(&remote_id)
            .unwrap()
            .to_string(),
        sid
    );
    let voice = app
        .state
        .store
        .conference_voice_for_session("ten_local", &sid)
        .unwrap()
        .unwrap();
    assert_eq!(voice.remote_connection_id, connid);
    // The complete scenario covers owner-initiated end. Here the telephone
    // participant hangs up and the core must retire the whole voice Session.
    hangup_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(10), remote)
        .await
        .unwrap()
        .unwrap();
    assert!(ended.load(Ordering::SeqCst));
    tokio::time::timeout(Duration::from_secs(5), async {
        while app
            .state
            .store
            .get_session("ten_local", &sid)
            .unwrap()
            .unwrap()
            .ended_at
            .is_none()
        {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("remote BYE must end the Session");
    assert_eq!(
        app.state
            .store
            .get_conversation("ten_local", &cid)
            .unwrap()
            .unwrap()
            .state,
        "open"
    );
    let end = assistant
        .request(command(MessageType::SessionEnd, Some(&cid), json!({})).with_sid(sid))
        .await;
    assert_eq!(end.msg_type, MessageType::Ack, "{}", end.payload);
    tokio::time::timeout(Duration::from_secs(5), provider)
        .await
        .unwrap()
        .unwrap();
    web.abort();
    api.abort();
}
