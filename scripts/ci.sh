#!/usr/bin/env bash
# Automated v1 gate. No live Vapi, Telnyx, or browser.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

cargo test
cargo test --features sip --test sip_loopback
cargo test --features media-webrtc --test talk_webrtc
