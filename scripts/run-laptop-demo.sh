#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export PARLEY_API_SECRET="${PARLEY_API_SECRET:-dev-only}"
export PARLEY_OPERATOR_BOOTSTRAP="${PARLEY_OPERATOR_BOOTSTRAP:-bootstrap}"
export PARLEY_VAPI_CHAT="${PARLEY_VAPI_CHAT:-fake}"
export PARLEY_SQLITE_PATH="${PARLEY_SQLITE_PATH:-$(mktemp -t parley.XXXXXX.sqlite)}"
cd "$ROOT"
echo "Parley laptop demo"
echo "  HTTP     http://127.0.0.1:8080"
echo "  Widget   http://127.0.0.1:8080/widget/"
echo "  Desk     http://127.0.0.1:8080/desk/"
echo "  UCTP WS  ws://127.0.0.1:7443"
echo "  SIP      127.0.0.1:5060"
echo "  sqlite   $PARLEY_SQLITE_PATH"
exec cargo run --offline --quiet --features sip,media-webrtc
