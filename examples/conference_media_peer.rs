//! Explicit synthetic SIP peer for a remote-browser media gate; never dials.
//! Usage: conference_media_peer /private/status.json
use rvoip_sip::{Config, Endpoint, EndpointAudioFrame, EndpointProfile};
use serde_json::json;
use std::{
    net::IpAddr,
    time::{Duration, Instant},
};

fn save(path: &str, state: &str, frames: u64, first: u128, total: u64) -> std::io::Result<()> {
    let temp = format!("{path}.tmp");
    std::fs::write(
        &temp,
        serde_json::to_vec(&json!({"state":state,"browser_tone_frames":frames,
        "first_browser_tone_ms":first,"received_frames":total,"synthetic":true}))?,
    )?;
    std::fs::rename(temp, path)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("private status file required")?;
    let mut cfg = Config::on("network-probe", "127.0.0.1".parse::<IpAddr>()?, 5094);
    cfg.media_port_start = 47400;
    cfg.media_port_end = 47500;
    // Match the deployed conference endpoint's single RTP/RTCP socket policy.
    cfg.rtcp_mux_required = true;
    let mut endpoint = Endpoint::builder()
        .name("network-probe")
        .profile(EndpointProfile::Custom(cfg))
        .build()
        .await?;
    save(&path, "listening", 0, 0, 0)?;
    let incoming =
        tokio::time::timeout(Duration::from_secs(120), endpoint.wait_for_incoming()).await??;
    let call = incoming.answer().await?;
    let (send, mut recv) = call.audio().await?.split();
    let mut tick = tokio::time::interval(Duration::from_millis(20));
    let mut flush = tokio::time::interval(Duration::from_millis(250));
    let (mut position, mut frames, mut first, mut total) = (0u32, 0u64, 0u128, 0u64);
    // Cloud qualification includes an AI hold and remote status queries before
    // takeover. Keep the synthetic peer alive through those bounded stages.
    let deadline = tokio::time::sleep(Duration::from_secs(360));
    tokio::pin!(deadline);
    let ended = call.wait_for_end(None);
    tokio::pin!(ended);
    let began = Instant::now();
    loop {
        tokio::select! {
            result = &mut ended => { result?; break; },
            _ = &mut deadline => { call.hangup().await?; break; },
            _ = flush.tick() => save(&path, "connected", frames, first, total)?,
            _ = tick.tick() => {
                let samples=(0..160).map(|i| (7000.0*(std::f64::consts::TAU*660.0*(position+i) as f64/8000.0).sin()) as i16).collect();
                if send.send(EndpointAudioFrame::new(samples,8000,1,position)).await.is_err(){break;}
                position+=160;
            },
            frame=recv.recv()=>match frame {
                Some(frame)=>{
                    total+=1;
                    let (mut sin,mut cos,mut energy)=(0.0,0.0,0.0);
                    for (i,s) in frame.samples.iter().enumerate(){
                        let phase=std::f64::consts::TAU*880.0*i as f64/frame.sample_rate as f64;
                        let v=*s as f64;sin+=v*phase.sin();cos+=v*phase.cos();energy+=v*v;
                    }
                    let purity=2.0*(sin*sin+cos*cos)/(frame.samples.len().max(1) as f64*energy.max(1.0));
                    if purity>0.8 && energy/frame.samples.len().max(1) as f64>250000.0 {
                        frames+=1;
                        if first==0 {first=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis();}
                    }
                },
                None=>{tokio::time::timeout(Duration::from_secs(5), &mut ended).await??;break;}
            }
        }
    }
    save(&path, "ended", frames, first, total)?;
    println!(
        "Synthetic SIP peer ended after {:?}; browser-tone frames: {}",
        began.elapsed(),
        frames
    );
    endpoint.shutdown().await?;
    Ok(())
}
