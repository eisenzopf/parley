#![cfg(all(
    feature = "uctp",
    feature = "sip",
    feature = "vapi",
    feature = "media-webrtc"
))]
//! Explicit live Vapi gate. SIP, participants, and speech are local test fixtures.
//! Kept in a separate test binary so the offline media gates never select it.
use parley::{store::Store, App, Config};
use rvoip_sip::{Config as SipConfig, Endpoint, EndpointAudioFrame, EndpointProfile};
use rvoip_uctp::{envelope::UctpEnvelope, types::MessageType};
use rvoip_vapi::{VapiAdapter, VapiApiKey, VapiConfig, VapiEvent};
use rvoip_websocket::UctpWsClient;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

struct Peer {
    client: Arc<UctpWsClient>,
    incoming: tokio::sync::mpsc::Receiver<UctpEnvelope>,
}
impl Peer {
    async fn connect(url: &url::Url, token: &str) -> Self {
        let client = UctpWsClient::connect(url).await.unwrap();
        let incoming = client.take_inbound().unwrap();
        let mut peer = Self { client, incoming };
        peer.client.send(UctpEnvelope::new(MessageType::AuthHello, json!({
            "device":{"id":"dev_live_voice","kind":"desktop","platform":"test","sdk_version":"test"},
            "auth_methods":["bearer"],"capabilities":{}
        }))).await.unwrap();
        let challenge = peer.next().await;
        assert_eq!(challenge.msg_type, MessageType::AuthChallenge);
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
        tokio::time::timeout(Duration::from_secs(10), self.incoming.recv())
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
    request.cid = cid.map(str::to_owned);
    request
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
#[ignore = "explicit live Vapi usage: PARLEY_LIVE_VOICE=1, VAPI_PRIVATE_KEY, CONFERENCE_SPEECH_PCM"]
async fn live_vapi_voice_roundtrip_over_uctp_and_local_sip() {
    assert_eq!(
        std::env::var("PARLEY_LIVE_VOICE").as_deref(),
        Ok("1"),
        "explicit live voice opt-in required"
    );
    let key = std::env::var("VAPI_PRIVATE_KEY").expect("VAPI_PRIVATE_KEY required");
    let assistant_id = std::env::var("VAPI_ASSISTANT_ID")
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
    let pcm = std::fs::read(
        std::env::var("CONFERENCE_SPEECH_PCM").expect("8 kHz mono s16le speech path required"),
    )
    .unwrap();
    assert!(
        pcm.len() > 16000 && pcm.len() <= 8000 * 2 * 45 && pcm.len() % 2 == 0,
        "supply 1–45 seconds of raw speech"
    );
    let speech: Vec<i16> = pcm
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect();
    assert!(
        speech.iter().any(|s| s.unsigned_abs() > 500),
        "speech fixture must be audible"
    );
    let _ = rustls::crypto::ring::default_provider().install_default();
    let adapter = VapiAdapter::new(VapiConfig::new(VapiApiKey::new(key.clone()).unwrap())).unwrap();
    let mut provider_events = adapter.subscribe_vapi_events();
    let provider_ended = Arc::new(AtomicBool::new(false));
    let observed_end = provider_ended.clone();
    let observer = tokio::spawn(async move {
        loop {
            let event = match provider_events.recv().await {
                Ok(event) => event,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            };
            if matches!(event.event, VapiEvent::StatusUpdate { ref status, .. } if status == "ended")
            {
                observed_end.store(true, Ordering::SeqCst);
            }
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::default();
    cfg.sqlite_path = dir.path().join("test.sqlite").display().to_string();
    cfg.blob_dir = dir.path().join("blobs").display().to_string();
    cfg.api_secret = "test-admin".into();
    cfg.vapi_chat_mode = "fake".into();
    cfg.vapi_assistant_id = assistant_id.clone();
    cfg.bind_uctp_ws = "127.0.0.1:0".into();
    cfg.telnyx_from = "+14155550000".into();
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    cfg.conference_sip_bind = Some(socket.local_addr().unwrap().to_string());
    drop(socket);
    let app = App::with_voice_adapter(cfg.clone(), Store::open(&cfg).unwrap(), adapter).unwrap();
    parley::conference_voice::bind(&app.state).await.unwrap();

    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let address = socket.local_addr().unwrap();
    drop(socket);
    let mut sip = SipConfig::on("organizer", address.ip(), address.port());
    sip.media_port_start = 47400;
    sip.media_port_end = 47600;
    let mut endpoint = Endpoint::builder()
        .name("organizer")
        .profile(EndpointProfile::Custom(sip))
        .build()
        .await
        .unwrap();
    let audible = Arc::new(AtomicUsize::new(0));
    let response_audio = Arc::new(AtomicUsize::new(0));
    let speech_sent = Arc::new(AtomicBool::new(false));
    let remote_ended = Arc::new(AtomicBool::new(false));
    let tone_mode = Arc::new(AtomicBool::new(false));
    let browser_audio = Arc::new(AtomicUsize::new(0));
    let browser_first_audio_ms = Arc::new(AtomicU64::new(0));
    let browser_first = browser_first_audio_ms.clone();
    let (tones, browser_frames) = (tone_mode.clone(), browser_audio.clone());
    let (heard, response, sent, ended) = (
        audible.clone(),
        response_audio.clone(),
        speech_sent.clone(),
        remote_ended.clone(),
    );
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
        let started = Instant::now();
        let mut last_audio = Instant::now();
        let mut speaking = false;
        let mut offset = 0;
        let mut timestamp = 0;
        let end = call.wait_for_end(None);
        tokio::pin!(end);
        loop {
            tokio::select! {
                result = &mut end => { result.unwrap(); ended.store(true,Ordering::SeqCst); break; },
                _ = tick.tick() => {
                    if !speaking && !sent.load(Ordering::SeqCst) && heard.load(Ordering::SeqCst) > 10
                        && (last_audio.elapsed() > Duration::from_millis(1000) || started.elapsed() > Duration::from_secs(20)) { speaking = true; }
                    let mut samples = vec![0;160];
                    if speaking {
                        let count = (speech.len() - offset).min(160);
                        samples[..count].copy_from_slice(&speech[offset..offset+count]); offset += count;
                        if offset == speech.len() { speaking = false; sent.store(true,Ordering::SeqCst); }
                    }
                    if tones.load(Ordering::SeqCst) {
                        for (i,sample) in samples.iter_mut().enumerate() {
                            *sample = (7000.0 * (std::f64::consts::TAU * 660.0 * (timestamp as f64 + i as f64) / 8000.0).sin()) as i16;
                        }
                    }
                    if send.send(EndpointAudioFrame::new(samples,8000,1,timestamp)).await.is_err() { break; }
                    timestamp += 160;
                },
                frame = recv.recv() => if let Some(frame) = frame {
                    if frame.samples.iter().any(|s| s.unsigned_abs() > 500) {
                        heard.fetch_add(1,Ordering::SeqCst); last_audio = Instant::now();
                        if sent.load(Ordering::SeqCst) { response.fetch_add(1,Ordering::SeqCst); }
                        let mut sin = 0.0; let mut cos = 0.0; let mut energy = 0.0;
                        for (i,sample) in frame.samples.iter().enumerate() {
                            let phase = std::f64::consts::TAU * 880.0 * i as f64 / frame.sample_rate as f64;
                            let value = *sample as f64;
                            sin += value * phase.sin(); cos += value * phase.cos(); energy += value * value;
                        }
                        let purity = 2.0 * (sin*sin+cos*cos) / (frame.samples.len().max(1) as f64 * energy.max(1.0));
                        if tones.load(Ordering::SeqCst) && purity > 0.8 {
                            browser_frames.fetch_add(1,Ordering::SeqCst);
                            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
                            let _ = browser_first.compare_exchange(0,now,Ordering::SeqCst,Ordering::SeqCst);
                        }
                    }
                } else { break; }
            }
        }
        if !ended.load(Ordering::SeqCst) {
            end.await.unwrap();
            ended.store(true, Ordering::SeqCst);
        }
    });
    let url = url::Url::parse(&format!("ws://{}", app.start_uctp().await.unwrap())).unwrap();
    let mut admin = Peer::connect(&url, "test-admin").await;
    let mut roster: Value = serde_json::from_str(include_str!(
        "../examples/conference-assistant/contacts.fixture.json"
    ))
    .unwrap();
    roster[3]["sip"] = json!(format!("sip:organizer@{address}"));
    let opened = admin
        .request(command(
            MessageType::ConversationCreate,
            None,
            json!({"participants":roster}),
        ))
        .await;
    assert_eq!(opened.msg_type, MessageType::ConversationOpened);
    let cid = opened.cid.unwrap();
    let owner_id = opened.payload["participants"][0]["participant_id"]
        .as_str()
        .unwrap();
    let target_id = opened.payload["participants"][3]["participant_id"]
        .as_str()
        .unwrap();
    let ai_id = opened.payload["participants"][4]["participant_id"]
        .as_str()
        .unwrap();
    let token = app
        .state
        .store
        .issue_conference_token("ten_local", &cid, owner_id)
        .unwrap();
    let mut owner = Peer::connect(&url, &token).await;
    let accepted = owner.request(command(MessageType::SessionInvite,Some(&cid),json!({"medium":"voice","to":target_id,
        "purpose":"This is a synthetic voice test. Ask the organizer to confirm sandbox pickup at terminal C. They will give a confirmation word; repeat that word back to them. No real travel purchase or SMS is authorized by this call."}))).await;
    assert_eq!(
        accepted.msg_type,
        MessageType::Ack,
        "voice invitation rejected"
    );
    let sid = accepted.payload["session"]["sid"]
        .as_str()
        .unwrap()
        .to_owned();
    // A WebSocket can close before a terminal event arrives. Verify the
    // authoritative provider resource, matched to this exact synthetic call.
    let provider_rest_ended = Arc::new(AtomicBool::new(false));
    let provider_matched = Arc::new(AtomicBool::new(false));
    let (rest_ended, matched) = (provider_rest_ended.clone(), provider_matched.clone());
    let (check_cid, check_sid) = (cid.clone(), sid.clone());
    let status_check = tokio::spawn(async move {
        let http = reqwest::Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let mut call_url: Option<url::Url> = None;
        let deadline = Instant::now() + Duration::from_secs(160);
        while Instant::now() < deadline {
            let request = if let Some(url) = &call_url {
                http.get(url.clone())
            } else {
                http.get("https://api.vapi.ai/call")
                    .query(&[("assistantId", assistant_id.as_str()), ("limit", "10")])
            };
            if let Ok(response) = request.bearer_auth(&key).send().await {
                if response.status().is_success() {
                    if let Ok(body) = response.json::<Value>().await {
                        let calls = if call_url.is_some() {
                            vec![body]
                        } else {
                            body.as_array().cloned().unwrap_or_default()
                        };
                        for call in calls {
                            if call["metadata"]["conversation_id"] != check_cid
                                || call["metadata"]["session_id"] != check_sid
                                || call["assistantId"] != assistant_id
                                || call["type"] != "vapi.websocketCall"
                            {
                                continue;
                            }
                            if let Some(id) = call["id"].as_str() {
                                let mut url = url::Url::parse("https://api.vapi.ai/call/").unwrap();
                                url.path_segments_mut().unwrap().pop_if_empty().push(id);
                                call_url = Some(url);
                                matched.store(true, Ordering::SeqCst);
                            }
                            if call["status"] == "ended" && call["endedAt"].is_string() {
                                rest_ended.store(true, Ordering::SeqCst);
                                return;
                            }
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
    let mut user_fact = false;
    let mut ai_fact = false;
    let mut attached = false;
    let result = tokio::time::timeout(Duration::from_secs(90), async {
        loop {
            let snapshot = owner
                .request(command(
                    MessageType::Unknown("conversation.subscribe".into()),
                    Some(&cid),
                    json!({"after":0}),
                ))
                .await;
            for event in snapshot.payload["events"].as_array().unwrap() {
                if event["event_type"] == "session.assistant_failed" {
                    return Err("Vapi attachment failed");
                }
                if event["event_type"] == "session.assistant_attached" {
                    attached = true;
                }
                if event["event_type"] != "session.transcript" {
                    continue;
                }
                let fact = &event["payload"];
                assert_eq!(event["cid"], cid);
                assert_eq!(fact["sid"], sid);
                assert_eq!(fact["source"], "vapi");
                assert_eq!(fact["is_final"], true);
                let text = fact["text"].as_str().unwrap_or("").to_lowercase();
                if text.contains("pineapple") {
                    user_fact |= fact["speaker"] == target_id;
                    ai_fact |= fact["speaker"] == ai_id;
                }
            }
            if user_fact && ai_fact && response_audio.load(Ordering::SeqCst) > 10 {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await;
    let reply_audio_before_handoff = response_audio.load(Ordering::SeqCst);
    let mut browser_output = None;
    if matches!(result, Ok(Ok(()))) {
        tone_mode.store(true, Ordering::SeqCst);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let http = format!("http://{}", listener.local_addr().unwrap());
        let (frames, ended, provider) = (
            browser_audio.clone(),
            remote_ended.clone(),
            provider_rest_ended.clone(),
        );
        let first_audio = browser_first_audio_ms.clone();
        let router = app.router().route("/__live_voice_probe",axum::routing::get(move || {
            let value = json!({"browser_audio_frames":frames.load(Ordering::SeqCst),"browser_first_audio_ms":first_audio.load(Ordering::SeqCst),"sip_ended":ended.load(Ordering::SeqCst),"provider_ended":provider.load(Ordering::SeqCst)});
            async move { axum::Json(value) }
        }));
        let web = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        browser_output = Some(tokio::time::timeout(Duration::from_secs(60),tokio::process::Command::new("node")
            .arg("e2e/conference-live-voice.mjs")
            .env("LIVE_VOICE_FIXTURE",json!({"http":http,"url":url.as_str(),"cid":cid,"token":token,"connid":accepted.payload["session"]["connid"]}).to_string())
            .kill_on_drop(true).output()).await);
        web.abort();
    }
    // Always request teardown before asserting probe success, including timeouts.
    let mut end = command(MessageType::SessionEnd, Some(&cid), json!({}));
    end.sid = Some(sid.clone());
    let closed = owner.request(end).await;
    let remote_result = tokio::time::timeout(Duration::from_secs(15), remote).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    observer.abort();
    status_check.abort();
    println!(
        "{}",
        json!({"event":"live.voice.probe","mode":"live Vapi voice; synthetic local SIP participant; no PSTN or SMS",
        "attached":attached,"speech_sent":speech_sent.load(Ordering::SeqCst),"audible_frames":audible.load(Ordering::SeqCst),
        "vapi_reply_audio_frames_before_handoff":reply_audio_before_handoff,"attributed_user_confirmation":user_fact,"attributed_assistant_confirmation":ai_fact,
        "provider_ended_event":provider_ended.load(Ordering::SeqCst),"provider_resource_matched":provider_matched.load(Ordering::SeqCst),
        "provider_resource_ended":provider_rest_ended.load(Ordering::SeqCst),"sip_ended":remote_ended.load(Ordering::SeqCst)})
    );
    assert_eq!(closed.msg_type, MessageType::Ack, "UCTP teardown rejected");
    remote_result
        .expect("SIP teardown timeout")
        .expect("SIP task failed");
    result
        .expect("live voice probe timed out")
        .expect("live voice probe failed");
    let browser_output = browser_output
        .expect("browser probe was not run")
        .expect("browser probe timeout")
        .expect("browser probe process failed");
    assert!(
        browser_output.status.success(),
        "browser handoff failed: {}\n{}",
        String::from_utf8_lossy(&browser_output.stdout),
        String::from_utf8_lossy(&browser_output.stderr)
    );
    println!("{}", String::from_utf8_lossy(&browser_output.stdout));
    assert!(
        browser_audio.load(Ordering::SeqCst) > 10,
        "SIP must receive browser audio"
    );
    assert!(
        provider_rest_ended.load(Ordering::SeqCst),
        "matched provider resource must confirm call termination"
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
    assert!(app
        .state
        .store
        .list_sessions("ten_local", &cid)
        .unwrap()
        .iter()
        .all(|s| s.state == "ended"));
}
