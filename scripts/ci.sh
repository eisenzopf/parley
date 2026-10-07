#!/usr/bin/env bash
# Automated v1 gate. No live Vapi, Telnyx, or Cloudflare.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

export PARLEY_VAPI_CHAT="${PARLEY_VAPI_CHAT:-fake}"
export PARLEY_TUNNEL="${PARLEY_TUNNEL:-0}"
export PARLEY_PROVISION="${PARLEY_PROVISION:-0}"
export CONFERENCE_LIVE_PLANNER=0
export CONFERENCE_LIVE_SCENARIO=0

node scripts/verify-conference-dependencies.mjs
npm ci
npm run test:conference-client
python3 -m unittest discover -s tests -p 'test_conference_kit.py'
cargo test --locked
cargo test --locked --features sip --test sip_loopback
cargo test --locked --features media-webrtc --test talk_webrtc
cargo build --locked

if command -v npx >/dev/null 2>&1; then
  if [ -n "${CI:-}" ]; then
    npx --yes playwright@1.55.1 install --with-deps chromium
  else
    npx --yes playwright@1.55.1 install chromium
  fi
  npx --yes playwright@1.55.1 test
  cargo test --locked --test uctp_conference -- --ignored --test-threads=1
fi
