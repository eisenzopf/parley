#!/usr/bin/env bash
# Live demo: Vapi + Telnyx + Cloudflare from the environment / `.env`.
# CI still uses fakes via PARLEY_VAPI_CHAT=fake PARLEY_TUNNEL=0 PARLEY_PROVISION=0.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export PARLEY_API_SECRET="${PARLEY_API_SECRET:-dev-only}"
export PARLEY_OPERATOR_BOOTSTRAP="${PARLEY_OPERATOR_BOOTSTRAP:-bootstrap}"
port_busy() {
  lsof -nP -iTCP:"$1" -sTCP:LISTEN >/dev/null 2>&1
}
if [ -z "${PARLEY_BIND_HTTP:-}" ] && port_busy 8080; then
  export PARLEY_BIND_HTTP="127.0.0.1:18080"
fi
if [ -z "${PARLEY_BIND_UCTP_WS:-}" ] && port_busy 7443; then
  export PARLEY_BIND_UCTP_WS="127.0.0.1:17443"
fi
if [ -z "${PARLEY_BIND_SIP:-}" ] && port_busy 5060; then
  export PARLEY_BIND_SIP="127.0.0.1:15060"
fi
cd "$ROOT"
echo "Parley live demo"
echo "  local    http://${PARLEY_BIND_HTTP:-127.0.0.1:8080}"
echo "  public   https://${PARLEY_HOSTNAME:-parley.rudeless.ai}"
echo "  sms      +18058253932"
exec cargo run --offline --quiet --features sip,media-webrtc,vapi
