#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
case "${1:-rehearsal}" in
  rehearsal|record-rehearsal)
    # Fresh test-owned database and loopback endpoints on every invocation.
    # Vapi and SMS fixtures are explicitly selected inside this test.
    node scripts/verify-conference-dependencies.mjs
    export CONFERENCE_LIVE_PLANNER=0 CONFERENCE_LIVE_SCENARIO=0
    if [ "${1:-rehearsal}" = record-rehearsal ]; then
      export CONFERENCE_RECORD=1
      echo 'Recording a labeled silent walkthrough with pauses for presenter narration.'
    fi
    echo 'Complete local rehearsal: simulated providers, real SIP and browser audio.'
    exec cargo test --locked --test uctp_conference complete_conference_scenario -- --ignored --nocapture
    ;;
  live-planner)
    # Only planning uses Vapi; this test hardcodes loopback voice and fake SMS.
    : "${VAPI_PRIVATE_KEY:?Set the Vapi private key for live planning}"
    node scripts/verify-conference-dependencies.mjs
    export CONFERENCE_LIVE_PLANNER=1 CONFERENCE_LIVE_SCENARIO=0
    echo 'Live Vapi planning (provider usage): simulated people, voice provider and SMS; real local SIP/browser audio.'
    exec cargo test --locked --test uctp_conference complete_conference_scenario -- --ignored --nocapture
    ;;
  live-vapi)
    : "${VAPI_PRIVATE_KEY:?Set the Vapi private key for live planning and voice}"
    node scripts/verify-conference-dependencies.mjs
    if [ -z "${CONFERENCE_SCENARIO_SPEECH_DIR:-}" ]; then
      node scripts/prepare-conference-speech.mjs scenario
      export CONFERENCE_SCENARIO_SPEECH_DIR=var/conference/scenario-speech
    fi
    export CONFERENCE_LIVE_PLANNER=1 CONFERENCE_LIVE_SCENARIO=1
    echo 'Live Vapi planning and voice (provider usage): synthetic local SIP participants and fixture SMS.'
    exec cargo test --locked --test uctp_conference complete_conference_scenario -- --ignored --nocapture
    ;;
  live-voice)
    : "${VAPI_PRIVATE_KEY:?Set the Vapi private key for live voice}"
    node scripts/verify-conference-dependencies.mjs
    if [ -z "${CONFERENCE_SPEECH_PCM:-}" ]; then
      node scripts/prepare-conference-speech.mjs
      export CONFERENCE_SPEECH_PCM=var/conference/voice-probe/organizer.pcm
    fi
    export PARLEY_LIVE_VOICE=1
    echo 'Live Vapi voice (provider usage): synthetic SIP speech, browser handoff, no PSTN or SMS delivery.'
    exec cargo test --locked --test conference_live_voice -- --ignored --nocapture
    ;;
  live-pstn)
    if [ -z "${2:-}" ]; then echo 'Supply the private PSTN contact JSON; this mode places one real call.' >&2; exit 2; fi
    node scripts/verify-conference-dependencies.mjs
    echo 'Real PSTN/browser takeover test: consenting telephone holder must be ready to answer.'
    exec node scripts/test-conference-pstn.mjs --run "$2"
    ;;
  live-server)
    # Reuses the specified durable state; does not provision provider resources.
    : "${PARLEY_SQLITE_PATH:?Set the intended rehearsal database path}"
    : "${PARLEY_API_SECRET:?Set the server admin secret}"
    if [ "$PARLEY_API_SECRET" = dev-only ]; then echo 'Choose a private admin secret for the live server.' >&2; exit 1; fi
    node scripts/verify-conference-dependencies.mjs
    export PARLEY_SQLITE="$PARLEY_SQLITE_PATH"
    export PARLEY_VAPI_CHAT=live PARLEY_PROVISION=0
    export PARLEY_TUNNEL="${PARLEY_TUNNEL:-0}"
    echo 'Starting live-provider server with the supplied environment/.env. Run preflight before launching the worker.'
    exec ./target/debug/parley
    ;;
  *) echo 'Usage: scripts/run-conference-demo.sh [rehearsal|record-rehearsal|live-planner|live-voice|live-vapi|live-pstn private-contact.json|live-server]' >&2; exit 2 ;;
esac
