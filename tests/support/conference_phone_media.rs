//! Synthetic telephone for real SIP/RTP answer-triggered phone moves.
use super::conference_media::{frequency, tone};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

pub struct PhoneEndpoint {
    pub address: std::net::SocketAddr,
    pub ringing: Arc<AtomicUsize>,
    pub answered: Arc<AtomicUsize>,
    pub ended: Arc<AtomicUsize>,
    pub remote_audio: Arc<AtomicUsize>,
    pub pending_audio: Arc<AtomicUsize>,
    pub pending_frames: Arc<AtomicUsize>,
    pub answer: tokio::sync::mpsc::Sender<()>,
    pub task: tokio::task::JoinHandle<()>,
}
pub async fn endpoint() -> PhoneEndpoint {
    use rvoip_sip::{Config, Endpoint, EndpointAudioFrame, EndpointProfile};
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let address = socket.local_addr().unwrap();
    drop(socket);
    let mut config = Config::on("owner-phone", address.ip(), address.port());
    config.media_port_start = 46200;
    config.media_port_end = 46300;
    let mut endpoint = Endpoint::builder()
        .name("owner-phone")
        .profile(EndpointProfile::Custom(config))
        .build()
        .await
        .unwrap();
    let ringing = Arc::new(AtomicUsize::new(0));
    let answered = Arc::new(AtomicUsize::new(0));
    let ended = Arc::new(AtomicUsize::new(0));
    let remote_audio = Arc::new(AtomicUsize::new(0));
    let pending_audio = Arc::new(AtomicUsize::new(0));
    let pending_frames = Arc::new(AtomicUsize::new(0));
    let (answer, mut answers) = tokio::sync::mpsc::channel(8);
    let (g, a, e, r) = (
        ringing.clone(),
        answered.clone(),
        ended.clone(),
        remote_audio.clone(),
    );
    let task = tokio::spawn(async move {
        for index in 0..2 {
            let incoming = endpoint.wait_for_incoming().await.unwrap();
            g.fetch_add(1, Ordering::SeqCst);
            if index == 0 {
                let guard = incoming.defer(Duration::from_secs(40));
                guard
                    .wait_for_cancelled(Some(Duration::from_secs(30)))
                    .await
                    .unwrap();
                e.fetch_add(1, Ordering::SeqCst);
                continue;
            }
            answers.recv().await.unwrap();
            let call = incoming.answer().await.unwrap();
            a.fetch_add(1, Ordering::SeqCst);
            let (send, mut recv) = call.audio().await.unwrap().split();
            let mut tick = tokio::time::interval(Duration::from_millis(20));
            let mut position = 0;
            let mut playout = rvoip_media_core::processing::audio::playout::PlayoutBuffer::new(
                rvoip_sip::PlayoutConfig {
                    adaptive: false,
                    ..Default::default()
                },
            );
            let end = call.wait_for_end(None);
            tokio::pin!(end);
            loop {
                tokio::select! {
                    result = &mut end => { result.unwrap(); break; },
                    _ = tick.tick() => {
                        while let Some(frame) = playout.pop_due(std::time::Instant::now()) {
                            if frame.samples.iter().any(|s| s.unsigned_abs()>500) && (frequency(&frame.samples,frame.sample_rate)-660.0).abs()<80.0 { r.fetch_add(1,Ordering::SeqCst); }
                        }
                        if send.send(EndpointAudioFrame::new(tone(1040.0,8000,position),8000,1,position)).await.is_err() { break; }
                        position += 160;
                    },
                    frame = recv.recv() => if let Some(frame) = frame { playout.push(frame.into(),std::time::Instant::now()); } else { end.await.unwrap(); break; },
                }
            }
            e.fetch_add(1, Ordering::SeqCst);
        }
    });
    PhoneEndpoint {
        address,
        ringing,
        answered,
        ended,
        remote_audio,
        pending_audio,
        pending_frames,
        answer,
        task,
    }
}
