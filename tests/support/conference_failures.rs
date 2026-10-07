//! Real loopback SIP failure paths driven through the conference UCTP profile.
use super::*;
use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::net::UdpSocket;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Outcome {
    Busy,
    Unanswered,
    Cancel,
    LateAnswer,
}
#[derive(Default)]
struct Wire {
    calls: std::sync::Mutex<HashSet<String>>,
    methods: std::sync::Mutex<Vec<String>>,
    invites: AtomicUsize,
    cancels: AtomicUsize,
    acks: AtomicUsize,
    byes: AtomicUsize,
}
fn header<'a>(packet: &'a str, name: &str) -> &'a str {
    packet
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case(name).then_some(value.trim())
        })
        .unwrap_or_else(|| panic!("fixture request missing {name}"))
}
fn response(request: &str, status: &str, address: std::net::SocketAddr, sdp: &str) -> String {
    let to = header(request, "To");
    let to = if to.contains(";tag=") {
        to.to_owned()
    } else {
        format!("{to};tag=failure-fixture")
    };
    let content = if sdp.is_empty() {
        String::new()
    } else {
        "Content-Type: application/sdp\r\n".into()
    };
    format!("SIP/2.0 {status}\r\nVia: {}\r\nFrom: {}\r\nTo: {to}\r\nCall-ID: {}\r\nCSeq: {}\r\nContact: <sip:fixture@{address}>\r\n{content}Content-Length: {}\r\n\r\n{sdp}",
        header(request,"Via"), header(request,"From"), header(request,"Call-ID"), header(request,"CSeq"),sdp.len())
}
async fn wait_until(mut condition: impl FnMut() -> bool, seconds: u64, detail: &str) {
    tokio::time::timeout(Duration::from_secs(seconds), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out: {detail}"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sip_busy_no_answer_cancel_and_late_answer_leave_no_active_call() {
    for outcome in [
        Outcome::Busy,
        Outcome::Unanswered,
        Outcome::Cancel,
        Outcome::LateAnswer,
    ] {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let remote = socket.local_addr().unwrap();
        let rtp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let sdp = format!("v=0\r\no=fixture 1 1 IN IP4 127.0.0.1\r\ns=Failure fixture\r\nc=IN IP4 127.0.0.1\r\nt=0 0\r\nm=audio {} RTP/AVP 0\r\na=rtpmap:0 PCMU/8000\r\na=sendrecv\r\n",rtp.local_addr().unwrap().port());
        let wire = Arc::new(Wire::default());
        let observed = wire.clone();
        let peer = tokio::spawn(async move {
            let mut bytes = [0u8; 16384];
            let mut invite = String::new();
            loop {
                let (length, sender) = socket.recv_from(&mut bytes).await.unwrap();
                let packet = std::str::from_utf8(&bytes[..length]).unwrap();
                let method = packet.split_whitespace().next().unwrap();
                observed.methods.lock().unwrap().push(method.to_owned());
                match method {
                    "INVITE" => {
                        observed.invites.fetch_add(1, Ordering::SeqCst);
                        observed
                            .calls
                            .lock()
                            .unwrap()
                            .insert(header(packet, "Call-ID").to_owned());
                        invite = packet.to_owned();
                        let status = if outcome == Outcome::Busy {
                            "486 Busy Here"
                        } else {
                            "180 Ringing"
                        };
                        socket
                            .send_to(response(packet, status, remote, "").as_bytes(), sender)
                            .await
                            .unwrap();
                    }
                    "CANCEL" => {
                        observed.cancels.fetch_add(1, Ordering::SeqCst);
                        socket
                            .send_to(response(packet, "200 OK", remote, "").as_bytes(), sender)
                            .await
                            .unwrap();
                        let (status, body) = if outcome == Outcome::LateAnswer {
                            ("200 OK", sdp.as_str())
                        } else {
                            ("487 Request Terminated", "")
                        };
                        socket
                            .send_to(response(&invite, status, remote, body).as_bytes(), sender)
                            .await
                            .unwrap();
                    }
                    "ACK" => {
                        observed.acks.fetch_add(1, Ordering::SeqCst);
                    }
                    "BYE" => {
                        socket
                            .send_to(response(packet, "200 OK", remote, "").as_bytes(), sender)
                            .await
                            .unwrap();
                        observed.byes.fetch_add(1, Ordering::SeqCst);
                    }
                    _ => {}
                }
            }
        });
        let reserved = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = config(dir.path());
        cfg.conference_sip_bind = Some(reserved.local_addr().unwrap().to_string());
        drop(reserved);
        cfg.conference_network.sip_media_ports = [47000, 47200];
        let app = App::new(cfg.clone(), Store::open(&cfg).unwrap()).unwrap();
        parley::conference_voice::bind(&app.state).await.unwrap();
        let url = url::Url::parse(&format!("ws://{}", app.start_uctp().await.unwrap())).unwrap();
        let mut admin = Peer::connect(&url, "test-admin").await;
        let (cid, members) = provision_with_sip(&mut admin, &format!("sip:fixture@{remote}")).await;
        let mut worker = member_peer(&app, &url, &cid, &members[4]).await;
        let invite = command(
            MessageType::SessionInvite,
            Some(&cid),
            json!({"medium":"voice","to":members[2].participant_id,"purpose":"Failure rehearsal"}),
        );
        let accepted = worker.request(invite.clone()).await;
        assert_eq!(
            accepted.msg_type,
            MessageType::Ack,
            "{outcome:?}: {}",
            accepted.payload
        );
        let sid = accepted.payload["session"]["sid"].as_str().unwrap();
        let connid = accepted.payload["session"]["connid"].as_str().unwrap();
        assert_eq!(worker.request(invite.clone()).await.id, accepted.id);
        wait_until(
            || wire.invites.load(Ordering::SeqCst) > 0,
            5,
            "first INVITE",
        )
        .await;
        if matches!(outcome, Outcome::Cancel | Outcome::LateAnswer) {
            // Wait for progress to ensure this is a wire CANCEL, not pre-dispatch cancellation.
            wait_until(
                || {
                    app.state
                        .store
                        .conference_events("ten_local", &cid, &members[0], 0, 500)
                        .unwrap()
                        .iter()
                        .any(|e| e.event_type == "connection.progress")
                },
                5,
                "ringing journal",
            )
            .await;
            let mut end = command(MessageType::SessionEnd, Some(&cid), json!({}));
            end.sid = Some(sid.into());
            let ended = worker.request(end.clone()).await;
            assert_eq!(
                ended.msg_type,
                MessageType::Ack,
                "{outcome:?}: {}",
                ended.payload
            );
            assert_eq!(worker.request(end).await.id, ended.id);
        }
        wait_until(
            || {
                app.state
                    .store
                    .get_session("ten_local", sid)
                    .unwrap()
                    .unwrap()
                    .ended_at
                    .is_some()
            },
            40,
            &format!("{outcome:?} Session terminal"),
        )
        .await;
        wait_until(
            || wire.acks.load(Ordering::SeqCst) > 0,
            5,
            "final response ACK",
        )
        .await;
        if outcome != Outcome::Busy {
            assert!(
                wire.cancels.load(Ordering::SeqCst) > 0,
                "{outcome:?} missing CANCEL"
            );
        }
        if outcome == Outcome::LateAnswer {
            wait_until(
                || wire.byes.load(Ordering::SeqCst) > 0,
                5,
                "late answer BYE",
            )
            .await;
        }
        if outcome == Outcome::LateAnswer {
            let methods = wire.methods.lock().unwrap();
            let cancel = methods.iter().position(|m| m == "CANCEL").unwrap();
            let ack = methods.iter().position(|m| m == "ACK").unwrap();
            let bye = methods.iter().position(|m| m == "BYE").unwrap();
            assert!(
                cancel < ack && ack < bye,
                "late answer must be ACKed before BYE: {methods:?}"
            );
        } else {
            assert_eq!(wire.byes.load(Ordering::SeqCst), 0);
        }
        let core_session = app
            .state
            .orchestrator
            .session(&rvoip_core::ids::SessionId::from_string(sid))
            .unwrap();
        wait_until(
            || core_session.read().unwrap().connections.is_empty(),
            5,
            "core detached all connections",
        )
        .await;
        assert!(matches!(
            core_session.read().unwrap().state,
            rvoip_core::session::SessionState::Ended | rvoip_core::session::SessionState::Failed
        ));
        assert_eq!(
            app.state
                .store
                .list_sessions("ten_local", &cid)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            wire.calls.lock().unwrap().len(),
            1,
            "replay created another SIP dialog"
        );
        assert_eq!(worker.request(invite).await.id, accepted.id);
        let connection = app
            .state
            .store
            .get_connection("ten_local", connid)
            .unwrap()
            .unwrap();
        assert!(
            matches!(connection.state.as_str(), "ended" | "failed"),
            "{outcome:?}: {}",
            connection.state
        );
        assert_eq!(
            app.state
                .store
                .count_live_voice_sessions("ten_local", &cid)
                .unwrap(),
            0
        );
        let events = app
            .state
            .store
            .conference_events("ten_local", &cid, &members[0], 0, 500)
            .unwrap();
        assert!(
            !events.iter().any(|e| matches!(
                e.event_type.as_str(),
                "connection.connected" | "session.assistant_attached"
            )),
            "{outcome:?} published an answered call"
        );
        let message = command(
            MessageType::MessageSend,
            Some(&cid),
            json!({"msg_id":format!("msg_after_{outcome:?}"),"to":[members[0].participant_id],"body":"Call did not complete; choose another option.","delivery":"chat"}),
        );
        assert_eq!(worker.request(message).await.msg_type, MessageType::Ack);
        assert_eq!(
            app.state
                .store
                .conference_preflight("ten_local", &cid)
                .unwrap()["ready_for_new_task"],
            true
        );
        println!("{outcome:?}: one SIP dialog, terminal Session, no connected/AI event, Conversation usable");
        peer.abort();
        let _ = peer.await;
    }
}
