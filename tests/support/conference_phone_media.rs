//! Synthetic telephone for actual SIP/RTP and DTMF phone-move coverage.
use super::conference_media::{frequency, tone};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

pub struct PhoneEndpoint {
    pub address: std::net::SocketAddr,
    pub answered: Arc<AtomicUsize>,
    pub ended: Arc<AtomicUsize>,
    pub remote_audio: Arc<AtomicUsize>,
    pub pending_audio: Arc<AtomicUsize>,
    pub pending_frames: Arc<AtomicUsize>,
    pub digits: tokio::sync::mpsc::Sender<char>,
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
    let answered = Arc::new(AtomicUsize::new(0));
    let ended = Arc::new(AtomicUsize::new(0));
    let remote_audio = Arc::new(AtomicUsize::new(0));
    let pending_audio = Arc::new(AtomicUsize::new(0));
    let pending_frames = Arc::new(AtomicUsize::new(0));
    let (digits, mut received_digits) = tokio::sync::mpsc::channel(8);
    let (a, e, r) = (answered.clone(), ended.clone(), remote_audio.clone());
    let (p, f) = (pending_audio.clone(), pending_frames.clone());
    let task = tokio::spawn(async move {
        // First callback exercises explicit cancellation; the second commits.
        for _ in 0..2 {
            let call = endpoint
                .wait_for_incoming()
                .await
                .unwrap()
                .answer()
                .await
                .unwrap();
            a.fetch_add(1, Ordering::SeqCst);
            let (send, mut recv) = call.audio().await.unwrap().split();
            let mut tick = tokio::time::interval(Duration::from_millis(20));
            let mut position = 0;
            let mut confirmed = false;
            let end = call.wait_for_end(None);
            tokio::pin!(end);
            let mut end_observed = false;
            loop {
                tokio::select! {
                result=&mut end=>{result.unwrap();end_observed=true;break},
                    digit=received_digits.recv()=>if let Some(digit)=digit {call.send_dtmf(digit).await.unwrap();if digit=='1'{confirmed=true;}},
                    _=tick.tick()=>{if send.send(EndpointAudioFrame::new(tone(1040.0,8000,position),8000,1,position)).await.is_err(){break}position+=160;},
                    frame=recv.recv()=>if let Some(frame)=frame {
                        if !confirmed {
                            f.fetch_add(1, Ordering::SeqCst);
                            if frame.samples.iter().any(|s|s.unsigned_abs()>500) {p.fetch_add(1, Ordering::SeqCst);}
                        }
                        if frame.samples.iter().any(|s|s.unsigned_abs()>500) && (frequency(&frame.samples,frame.sample_rate)-660.0).abs()<80.0 {r.fetch_add(1,Ordering::SeqCst);}
                    } else {break},
                }
            }
            if !end_observed {
                end.await.unwrap();
            }
            e.fetch_add(1, Ordering::SeqCst);
        }
    });
    PhoneEndpoint {
        address,
        answered,
        ended,
        remote_audio,
        pending_audio,
        pending_frames,
        digits,
        task,
    }
}
