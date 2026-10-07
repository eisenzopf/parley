//! Bounded callback prompt on Rvoip-core's negotiated media stream.
//! Owns no inbound receiver; the later speaking bridge can still acquire it.
use base64::{engine::general_purpose::STANDARD, Engine};
use bytes::Bytes;
use rvoip_core::{
    bridge::peer_switch::PeerRouteTicket,
    capability::CodecInfo,
    peer_media::PeerMediaFrame,
    stream::{MediaFrame, MediaStream, StreamKind},
};
use std::sync::Arc;
use tokio::{sync::mpsc, time::Instant};

pub(crate) struct PhonePrompt {
    stream: Arc<dyn MediaStream>,
    sender: mpsc::Sender<PeerMediaFrame>,
    ticket: PeerRouteTicket,
    audio: Vec<u8>,
    payload_type: u8,
    position: usize,
    timestamp: u32,
}

fn encoded_prompt(codec: &CodecInfo) -> Result<(Vec<u8>, u8, u8), &'static str> {
    if codec.clock_rate_hz != 8000 || codec.channels != 1 {
        return Err("callback prompt requires negotiated mono G.711 at 8 kHz");
    }
    let (field, silence, payload_type) = match rvoip_core::bridge::codec_to_pt(&codec.name) {
        Some(0) if codec.payload_type.is_none_or(|pt| pt == 0) => ("pcmu_base64", 0xff, 0),
        Some(8) if codec.payload_type.is_none_or(|pt| pt == 8) => ("pcma_base64", 0xd5, 8),
        _ => return Err("callback prompt codec is unsupported"),
    };
    let asset: serde_json::Value =
        serde_json::from_str(include_str!("../config/conference-phone-prompt.json"))
            .map_err(|_| "callback prompt asset is invalid")?;
    let mut audio = STANDARD
        .decode(
            asset[field]
                .as_str()
                .ok_or("callback prompt asset is missing")?,
        )
        .map_err(|_| "callback prompt asset is invalid")?;
    if !(8000..=120000).contains(&audio.len()) {
        return Err("callback prompt asset duration is invalid");
    }
    // Complete the final 20 ms frame, then four seconds of actual G.711
    // silence. Repeat until confirmation/cancellation; never send an
    // unnegotiated comfort-noise payload type.
    let padded = audio.len().div_ceil(160) * 160;
    audio.resize(padded + 4 * 8000, silence);
    Ok((audio, silence, payload_type))
}

impl PhonePrompt {
    pub(crate) fn new(stream: Arc<dyn MediaStream>) -> Result<Self, &'static str> {
        let (audio, _, payload_type) = encoded_prompt(&stream.codec())?;
        let sender = stream
            .try_peer_frames_out()
            .map_err(|_| "callback prompt stream is not writable")?;
        Ok(Self {
            stream,
            sender,
            ticket: PeerRouteTicket::initial(),
            audio,
            payload_type,
            position: 0,
            timestamp: 0,
        })
    }

    pub(crate) async fn send_next(&mut self, deadline: Instant) -> Result<(), &'static str> {
        let codec = self.stream.codec();
        if codec.clock_rate_hz != 8000
            || codec.channels != 1
            || rvoip_core::bridge::codec_to_pt(&codec.name) != Some(self.payload_type)
            || codec.payload_type.is_some_and(|pt| pt != self.payload_type)
        {
            return Err("callback prompt codec changed while waiting");
        }
        let payload = Bytes::copy_from_slice(&self.audio[self.position..self.position + 160]);
        let frame = MediaFrame {
            stream_id: self.stream.id(),
            kind: StreamKind::Audio,
            payload,
            timestamp_rtp: self.timestamp,
            captured_at: chrono::Utc::now(),
            payload_type: Some(self.payload_type),
        };
        tokio::time::timeout_at(
            deadline,
            self.sender
                .send(PeerMediaFrame::new(frame, self.ticket.clone())),
        )
        .await
        .map_err(|_| "callback prompt send timed out")?
        .map_err(|_| "callback prompt stream ended")?;
        self.position = (self.position + 160) % self.audio.len();
        self.timestamp = self.timestamp.wrapping_add(160);
        Ok(())
    }

    pub(crate) async fn stop(self, deadline: Instant) -> Result<(), &'static str> {
        // Fence sends already dequeued by the transport and invalidate queued
        // prompt frames before the speaking bridge starts. No prompt may leak
        // into the joined conversation through a legacy unguarded queue.
        let guard = tokio::time::timeout_at(deadline, self.ticket.quiesce())
            .await
            .map_err(|_| "callback prompt cutoff timed out")?
            .ok_or("callback prompt cutoff failed")?;
        self.ticket.retire_if_current();
        drop(guard);
        Ok(())
    }
}

impl Drop for PhonePrompt {
    fn drop(&mut self) {
        self.ticket.retire_if_current();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_supports_only_negotiated_g711_and_has_a_continuous_quiet_tail() {
        for (name, pt, silence) in [
            ("PCMU", 0, 0xff),
            ("g.711-mu", 0, 0xff),
            ("PCMA", 8, 0xd5),
            ("g.711-a", 8, 0xd5),
        ] {
            let mut codec = CodecInfo {
                name: name.into(),
                clock_rate_hz: 8000,
                channels: 1,
                fmtp: None,
                payload_type: Some(pt),
            };
            let (audio, actual_silence, actual_pt) = encoded_prompt(&codec).unwrap();
            assert_eq!((actual_pt, actual_silence), (pt, silence));
            assert_eq!(audio.len() % 160, 0);
            assert!(audio[..audio.len() - 32000].iter().any(|&b| b != silence));
            assert!(audio[audio.len() - 32000..].iter().all(|&b| b == silence));
            codec.payload_type = Some(111);
            assert!(encoded_prompt(&codec).is_err());
        }
        assert!(encoded_prompt(&CodecInfo {
            name: "opus".into(),
            clock_rate_hz: 48000,
            channels: 2,
            fmtp: None,
            payload_type: Some(111)
        })
        .is_err());
    }
}
