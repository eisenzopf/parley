//! Complete clean-database story with two real SIP dialogs and real Chrome.
//! Vapi decisions/transcripts and SMS default to local provider fixtures.
//! CONFERENCE_LIVE_PLANNER=1 opts into real Chat decisions, with voice/SMS still local.
//! CONFERENCE_LIVE_SCENARIO=1 also selects real Vapi voice and synthetic SIP speech.
use super::conference_media::{frequency, tone};
use super::*;
use futures_util::{SinkExt, StreamExt};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[derive(Default)]
struct Remote {
    ai_audio: AtomicUsize,
    browser_audio: AtomicUsize,
    phone_audio: AtomicUsize,
    ended: AtomicBool,
    speech_sent: AtomicBool,
    tone_mode: AtomicBool,
}
async fn endpoint(
    name: &str,
    ports: [u16; 2],
    speech: Option<Vec<i16>>,
) -> (
    std::net::SocketAddr,
    Arc<Remote>,
    tokio::task::JoinHandle<()>,
) {
    use rvoip_sip::{Config as SipConfig, Endpoint, EndpointAudioFrame, EndpointProfile};
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = socket.local_addr().unwrap();
    drop(socket);
    let mut cfg = SipConfig::on(name, addr.ip(), addr.port());
    cfg.media_port_start = ports[0];
    cfg.media_port_end = ports[1];
    let mut endpoint = Endpoint::builder()
        .name(name)
        .profile(EndpointProfile::Custom(cfg))
        .build()
        .await
        .unwrap();
    let stats = Arc::new(Remote::default());
    let observed = stats.clone();
    let task = tokio::spawn(async move {
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
        let started = std::time::Instant::now();
        let mut last_audio = std::time::Instant::now();
        let mut speaking = false;
        let mut offset = 0;
        let end = call.wait_for_end(None);
        tokio::pin!(end);
        loop {
            tokio::select! {
                ended=&mut end=>{ended.unwrap();observed.ended.store(true,Ordering::SeqCst);break;},
                _=tick.tick()=>{
                    let samples = if let Some(speech) = &speech {
                        let mut samples = vec![0;160];
                        if !speaking && !observed.speech_sent.load(Ordering::SeqCst) && observed.ai_audio.load(Ordering::SeqCst)>10
                            && (last_audio.elapsed()>Duration::from_secs(1) || started.elapsed()>Duration::from_secs(20)) { speaking=true; }
                        if speaking {
                            let count = (speech.len()-offset).min(160);
                            samples[..count].copy_from_slice(&speech[offset..offset+count]); offset+=count;
                            if offset==speech.len() {speaking=false;observed.speech_sent.store(true,Ordering::SeqCst);}
                        }
                        if observed.tone_mode.load(Ordering::SeqCst) {tone(660.0,8000,position)} else {samples}
                    } else {tone(660.0,8000,position)};
                    if send.send(EndpointAudioFrame::new(samples,8000,1,position)).await.is_err(){break;}
                    position+=160;
                },
                frame=recv.recv()=>if let Some(frame)=frame {
                    if frame.samples.iter().any(|sample|sample.unsigned_abs()>500) {
                        last_audio=std::time::Instant::now();
                        let hz=frequency(&frame.samples,frame.sample_rate);
                        if (hz-1040.0).abs()<80.0 {observed.phone_audio.fetch_add(1,Ordering::SeqCst);}
                        if speech.is_some() && !observed.tone_mode.load(Ordering::SeqCst) || speech.is_none() && (hz-440.0).abs()<80.0 {observed.ai_audio.fetch_add(1,Ordering::SeqCst);}
                        if speech.is_none() && (hz-880.0).abs()<80.0 {observed.browser_audio.fetch_add(1,Ordering::SeqCst);}
                        if speech.is_some() && observed.tone_mode.load(Ordering::SeqCst) {
                            let mut sin=0.0;let mut cos=0.0;let mut energy=0.0;
                            for (i,sample) in frame.samples.iter().enumerate() {
                                let phase=std::f64::consts::TAU*880.0*i as f64/frame.sample_rate as f64;
                                let value=*sample as f64;sin+=value*phase.sin();cos+=value*phase.cos();energy+=value*value;
                            }
                            if 2.0*(sin*sin+cos*cos)/(frame.samples.len().max(1) as f64*energy.max(1.0))>0.8 {observed.browser_audio.fetch_add(1,Ordering::SeqCst);}
                        }
                    }
                } else {break;}
            }
        }
        if !observed.ended.load(Ordering::SeqCst) {
            end.await.unwrap();
            observed.ended.store(true, Ordering::SeqCst);
        }
    });
    (addr, stats, task)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
#[ignore = "requires npm ci and Playwright browser; explicit complete-scenario gate"]
async fn complete_conference_scenario_uses_external_worker_and_stage_ui() {
    use tokio_tungstenite::tungstenite::Message;
    if std::env::var("CONFERENCE_TRACE_LIFECYCLE").as_deref() == Ok("1") {
        let filter = if std::env::var("CONFERENCE_VAPI_DIAGNOSTICS").as_deref() == Ok("1") {
            "parley::conference_voice=debug,rvoip_vapi=debug"
        } else {
            "parley::conference_voice=debug,rvoip_vapi=warn"
        };
        let _ = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_test_writer()
            .try_init();
    }
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config(dir.path());
    cfg.vapi_assistant_id = "fixture-assistant".into();
    let live_voice = std::env::var("CONFERENCE_LIVE_SCENARIO").as_deref() == Ok("1");
    let live_key = if live_voice {
        assert_eq!(
            std::env::var("CONFERENCE_LIVE_PLANNER").as_deref(),
            Ok("1"),
            "combined gate requires real planning"
        );
        cfg.vapi_assistant_id = std::env::var("VAPI_ASSISTANT_ID")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                let saved: Value = serde_json::from_slice(
                    &std::fs::read("var/provision.json")
                        .expect("assistant ID or provision cache required"),
                )
                .unwrap();
                saved["assistant_id"]
                    .as_str()
                    .expect("cached assistant ID required")
                    .to_owned()
            });
        Some(std::env::var("VAPI_PRIVATE_KEY").expect("VAPI_PRIVATE_KEY required"))
    } else {
        None
    };
    let speech = |role: &str| {
        if live_voice {
            let path = std::path::PathBuf::from(
                std::env::var("CONFERENCE_SCENARIO_SPEECH_DIR")
                    .expect("scenario speech directory required"),
            )
            .join(format!("{role}.pcm"));
            let bytes = std::fs::read(path).expect("raw 8 kHz mono s16le fixture required");
            assert!(
                bytes.len() > 16000 && bytes.len() <= 720000 && bytes.len() % 2 == 0,
                "1–45 seconds of raw speech required"
            );
            Some(
                bytes
                    .chunks_exact(2)
                    .map(|b| i16::from_le_bytes([b[0], b[1]]))
                    .collect(),
            )
        } else {
            None
        }
    };
    let booker_speech = speech("booker");
    let organizer_speech = speech("organizer");
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    cfg.conference_sip_bind = Some(socket.local_addr().unwrap().to_string());
    drop(socket);
    let scenario: Value = serde_json::from_str(
        if std::env::var("CONFERENCE_DEMO_MODE").as_deref() == Ok("voice-only") {
            include_str!("../../config/conference-voice-scenario.json")
        } else {
            include_str!("../../config/conference-scenario.json")
        },
    )
    .unwrap();
    let transcripts = [
        scenario["booker_transcript"].as_str().unwrap().to_owned(),
        scenario["organizer_transcript"]
            .as_str()
            .unwrap()
            .to_owned(),
    ];
    let audio_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let audio_url = format!("ws://{}/audio", audio_listener.local_addr().unwrap());
    let provider_received = Arc::new([AtomicUsize::new(0), AtomicUsize::new(0)]);
    let provider_observed = provider_received.clone();
    let provider = tokio::spawn(async move {
        let mut calls = vec![];
        for (index, transcript) in transcripts.into_iter().enumerate() {
            let (tcp, _) = audio_listener.accept().await.unwrap();
            let counts = provider_observed.clone();
            calls.push(tokio::spawn(async move {
                let mut ws=tokio_tungstenite::accept_async(tcp).await.unwrap();
                let mut tick=tokio::time::interval(Duration::from_millis(20));let mut position=0;
                loop {
                    tokio::select! {
                        _=tick.tick()=>{
                            let pcm:Vec<u8>=tone(440.0,16000,position).into_iter().flat_map(i16::to_le_bytes).collect();position+=320;
                            if ws.send(Message::Binary(pcm.into())).await.is_err(){break;}
                            if position==16000 {
                                if ws.send(Message::Text(json!({"type":"transcript","role":"user","transcriptType":"final","transcript":transcript}).to_string().into())).await.is_err(){break;}
                                if index==1 {
                                    if ws.send(Message::Text(json!({"type":"tool-calls","toolCallList":[{"id":"fixture-browser-join","function":{"name":"request_browser_join","arguments":"{}"}}]}).to_string().into())).await.is_err(){break;}
                                }
                            }
                        },
                        frame=ws.next()=>match frame {
                            Some(Ok(Message::Binary(bytes)))=>{
                                let samples:Vec<i16>=bytes.chunks_exact(2).map(|s|i16::from_le_bytes([s[0],s[1]])).collect();
                                if samples.iter().any(|s|s.abs()>500) && (frequency(&samples,16000)-660.0).abs()<100.0 {counts[index].fetch_add(1,Ordering::SeqCst);}
                            },
                            Some(Ok(Message::Text(text))) if text.contains("end-call")=>{let _=ws.send(Message::Text(r#"{"type":"status-update","status":"ended"}"#.into())).await;break;},
                            Some(Ok(Message::Close(_)))|Some(Err(_))|None=>break,
                            _=>{},
                        }
                    }
                }
            }));
        }
        for call in calls {
            call.await.unwrap();
        }
    });
    let voice_api = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let api_url = url::Url::parse(&format!("http://{}/", voice_api.local_addr().unwrap())).unwrap();
    let call_count = Arc::new(AtomicUsize::new(0));
    let created = call_count.clone();
    let api = tokio::spawn(async move {
        let router=axum::Router::new().route("/call",axum::routing::post(move |axum::Json(body):axum::Json<Value>| {
            let n=created.fetch_add(1,Ordering::SeqCst);let audio_url=audio_url.clone();
            async move {
                assert_eq!(body["assistantId"],"fixture-assistant");
                assert!(body["assistantOverrides"]["firstMessage"].as_str().unwrap().contains("I'm David, Jonathan's AI assistant"));
                assert!(body["assistantOverrides"]["model"]["messages"][0]["content"].as_str().unwrap().contains("You are David, Jonathan's AI"));
                assert_eq!(body["assistantOverrides"]["silenceTimeoutSeconds"], if n == 0 { 30 } else { 180 }, "organizer must remain connected during coordinator/browser waiting");
                assert_eq!(body["assistantOverrides"]["maxDurationSeconds"], 600);
                let prompt = body["assistantOverrides"]["model"]["messages"][0]["content"].as_str().unwrap();
                if n == 0 {
                    assert!(prompt.contains("When the reservationist says goodbye"));
                    assert_eq!(body["assistantOverrides"]["model"]["tools"],json!([]));
                } else {
                    assert!(prompt.contains("invoke request_browser_join exactly once"));
                    assert_eq!(body["assistantOverrides"]["model"]["tools"][0]["function"]["name"],"request_browser_join");
                    assert_eq!(body["assistantOverrides"]["model"]["tools"][0]["async"],true);
                    assert!(body["assistantOverrides"]["model"]["tools"][0].get("server").is_none());
                    assert!(!prompt.contains("ask what the reservationist can offer"));
                }
                assert!(n<2,"no duplicate provider calls");
                axum::Json(json!({"id":format!("fixture-call-{n}"),"transport":{"websocketCallUrl":audio_url}}))
            }
        }));
        axum::serve(voice_api, router).await.unwrap();
    });
    let _ = rustls::crypto::ring::default_provider().install_default();
    let voice_config = if let Some(key) = &live_key {
        rvoip_vapi::VapiConfig::new(rvoip_vapi::VapiApiKey::new(key.clone()).unwrap())
    } else {
        rvoip_vapi::VapiConfig::new(rvoip_vapi::VapiApiKey::new("fixture-key").unwrap())
            .with_api_base(api_url)
            .with_loopback_test_transport()
    };
    let voice = rvoip_vapi::VapiAdapter::new(voice_config).unwrap();
    let app = App::with_voice_adapter(cfg.clone(), Store::open(&cfg).unwrap(), voice).unwrap();
    if std::env::var("CONFERENCE_TRACE_LIFECYCLE").as_deref() == Ok("1") {
        let mut events = app.state.orchestrator.subscribe_events();
        tokio::spawn(async move {
            while let Ok(event) = events.recv().await {
                use rvoip_core::events::Event;
                match event {
                    Event::ConnectionEnded {
                        connection_id,
                        reason,
                        ..
                    } => eprintln!("fixture connection ended: {connection_id} {reason:?}"),
                    Event::ConnectionFailed {
                        connection_id,
                        detail,
                        ..
                    } => eprintln!("fixture connection failed: {connection_id} {detail}"),
                    Event::SessionEnded { session_id, .. } => {
                        eprintln!("fixture session ended: {session_id}")
                    }
                    Event::SessionFailed {
                        session_id, detail, ..
                    } => eprintln!("fixture session failed: {session_id} {detail}"),
                    _ => {}
                }
            }
        });
    }
    parley::conference_voice::bind(&app.state).await.unwrap();
    let (booker_addr, booker, booker_task) =
        endpoint("booker", [45600, 45700], booker_speech).await;
    let (organizer_addr, organizer, organizer_task) =
        endpoint("organizer", [45800, 45900], organizer_speech).await;
    let phone = super::conference_phone_media::endpoint().await;
    let url = url::Url::parse(&format!("ws://{}", app.start_uctp().await.unwrap())).unwrap();
    let mut admin = Peer::connect(&url, "test-admin").await;
    let mut roster: Value = serde_json::from_str(include_str!(
        "../../examples/conference-assistant/contacts.fixture.json"
    ))
    .unwrap();
    if std::env::var("CONFERENCE_DEMO_MODE").as_deref() == Ok("voice-only") {
        for member in roster.as_array_mut().unwrap() {
            member.as_object_mut().unwrap().remove("sms");
        }
    }
    roster[2]["sip"] = json!(format!("sip:booker@{booker_addr}"));
    roster[0]["sip"] = json!(format!("sip:owner-phone@{}", phone.address));
    roster[3]["sip"] = json!(format!("sip:organizer@{organizer_addr}"));
    let created = admin
        .request(command(
            MessageType::ConversationCreate,
            None,
            json!({"participants":roster}),
        ))
        .await;
    assert_eq!(created.msg_type, MessageType::ConversationOpened);
    let cid = created.cid.unwrap();
    let members: Vec<Member> =
        serde_json::from_value(created.payload["participants"].clone()).unwrap();
    let owner_token = app
        .state
        .store
        .issue_conference_token("ten_local", &cid, &members[0].participant_id)
        .unwrap();
    let assistant_token = app
        .state
        .store
        .issue_conference_token("ten_local", &cid, &members[4].participant_id)
        .unwrap();
    let http = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let http_url = format!("http://{}", http.local_addr().unwrap());
    let observed = organizer.clone();
    let tones = organizer.clone();
    let phone_answered = phone.answered.clone();
    let phone_ended = phone.ended.clone();
    let phone_remote = phone.remote_audio.clone();
    let phone_pending_audio = phone.pending_audio.clone();
    let phone_pending_frames = phone.pending_frames.clone();
    let digits = phone.digits.clone();
    let router=app.router().route("/__scenario_fixture",axum::routing::get(move || {
        let value=json!({"ai_frames":observed.ai_audio.load(Ordering::SeqCst),"human_frames":observed.browser_audio.load(Ordering::SeqCst),"phone_frames":observed.phone_audio.load(Ordering::SeqCst),"owner_received_frames":phone_remote.load(Ordering::SeqCst),"phone_pending_audio":phone_pending_audio.load(Ordering::SeqCst),"phone_pending_frames":phone_pending_frames.load(Ordering::SeqCst),"phone_answered":phone_answered.load(Ordering::SeqCst),"phone_ended":phone_ended.load(Ordering::SeqCst),"ended":observed.ended.load(Ordering::SeqCst),"speech_sent":observed.speech_sent.load(Ordering::SeqCst)});
        async move {axum::Json(value)}
    })).route("/__scenario_tone",axum::routing::post(move || {
        let tones=tones.clone();async move {tones.tone_mode.store(true,Ordering::SeqCst);axum::Json(json!({"synthetic_tone":true}))}
    })).route("/__scenario_phone_digit",axum::routing::post(move |axum::Json(body):axum::Json<Value>| {
        let digits=digits.clone();async move {let digit=body["digit"].as_str().unwrap().chars().next().unwrap();digits.send(digit).await.unwrap();axum::Json(json!({"fixture_digit_sent":true}))}
    }));
    let web = tokio::spawn(async move {
        axum::serve(http, router).await.unwrap();
    });
    let scenario_timeout = if std::env::var("CONFERENCE_LIVE_PLANNER").as_deref() == Ok("1") {
        360
    } else {
        120
    };
    let output=tokio::time::timeout(Duration::from_secs(scenario_timeout),tokio::process::Command::new("node").arg("e2e/conference-scenario.mjs")
        .env("CONFERENCE_SCENARIO_FIXTURE",json!({"http":http_url,"url":url.as_str(),"cid":cid,"owner_token":owner_token,"assistant_token":assistant_token,"worker_state":dir.path().join("worker.json"),"scenario":scenario,"live_voice":live_voice}).to_string())
        .kill_on_drop(true).output()).await;
    // A failed browser/model gate must still retire any real provider calls.
    if live_voice {
        let mut owner = member_peer(&app, &url, &cid, &members[0]).await;
        for session in app.state.store.list_sessions("ten_local", &cid).unwrap() {
            if session.state != "ended" && session.state != "failed" {
                let mut end = command(MessageType::SessionEnd, Some(&cid), json!({}));
                end.sid = Some(session.id);
                let _ = owner.request(end).await;
            }
        }
        provider.abort();
    }
    let sessions = app.state.store.list_sessions("ten_local", &cid).unwrap();
    let provider_proof = if let Some(key) = &live_key {
        Some(
            super::vapi_provider::verify_ended_calls(
                key,
                &cfg.vapi_assistant_id,
                &cid,
                &sessions.iter().map(|s| s.id.clone()).collect::<Vec<_>>(),
            )
            .await,
        )
    } else {
        None
    };
    let output = output
        .expect("scenario deadline exceeded")
        .expect("scenario process failed");
    assert!(
        output.status.success(),
        "complete scenario failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    println!("{}", String::from_utf8_lossy(&output.stdout));
    for handle in [booker_task, organizer_task] {
        tokio::time::timeout(Duration::from_secs(10), handle)
            .await
            .unwrap()
            .unwrap();
    }
    assert!(booker.ended.load(Ordering::SeqCst) && organizer.ended.load(Ordering::SeqCst));
    assert!(
        booker.ai_audio.load(Ordering::SeqCst) > 10
            && organizer.ai_audio.load(Ordering::SeqCst) > 10
    );
    assert!(organizer.browser_audio.load(Ordering::SeqCst) > 10);
    assert!(
        organizer.phone_audio.load(Ordering::SeqCst) > 10
            && phone.remote_audio.load(Ordering::SeqCst) > 10,
        "telephone-to-telephone audio required in both directions"
    );
    tokio::time::timeout(Duration::from_secs(10), phone.task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        phone.answered.load(Ordering::SeqCst),
        2,
        "cancelled + successful callback, no duplicates"
    );
    assert_eq!(
        phone.ended.load(Ordering::SeqCst),
        2,
        "all callback resources ended"
    );
    if live_voice {
        assert_eq!(
            provider_proof
                .unwrap()
                .expect("live provider teardown verification"),
            2
        );
        println!(
            "{}",
            json!({"event":"complete.scenario.provider_resources","matched_ended_calls":2,"live_planning":true,"live_voice":true,"sms":"fixture"})
        );
        assert!(
            booker.speech_sent.load(Ordering::SeqCst)
                && organizer.speech_sent.load(Ordering::SeqCst),
            "both stand-ins must finish their spoken facts"
        );
    } else {
        tokio::time::timeout(Duration::from_secs(10), provider)
            .await
            .unwrap()
            .unwrap();
        assert!(provider_received
            .iter()
            .all(|count| count.load(Ordering::SeqCst) > 10));
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }
    let sessions = app.state.store.list_sessions("ten_local", &cid).unwrap();
    assert_eq!(sessions.len(), 2);
    assert!(sessions.iter().all(|s| s.state == "ended"));
    assert_eq!(
        app.state
            .store
            .get_conversation("ten_local", &cid)
            .unwrap()
            .unwrap()
            .state,
        "open"
    );
    let deliveries = app
        .state
        .store
        .conference_deliveries("ten_local", &cid)
        .unwrap();
    let voice_only = std::env::var("CONFERENCE_DEMO_MODE").as_deref() == Ok("voice-only");
    assert_eq!(deliveries.len(), if voice_only { 0 } else { 5 });
    assert!(deliveries.iter().all(|d| d.state == "sent"));
    let finals: Vec<_> = deliveries
        .iter()
        .filter(|d| d.body.starts_with("Rudeless Thelve: [Sandbox arrangements]"))
        .collect();
    assert_eq!(finals.len(), if voice_only { 0 } else { 4 });
    for member in &members[..4] {
        assert_eq!(
            finals
                .iter()
                .filter(|d| d.participant_id == member.participant_id)
                .count(),
            if voice_only { 0 } else { 1 }
        );
    }
    web.abort();
    api.abort();
}
