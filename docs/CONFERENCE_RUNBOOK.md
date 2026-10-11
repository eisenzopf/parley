# Conference demo runbook

Current voice-only rehearsal: [steps and operator setup](CONFERENCE_VOICE_REHEARSAL.md).
The carrier-approved full flow uses enrolled SMS recipients. Voice-only mode remains available for rehearsals that defer SMS.

Conference: October 13–15, 2026. Freeze the release on October 11 and rehearse
October 12. The exact speaking slot and demo duration are still unconfirmed.
This runbook covers the implemented complete local rehearsal and the pending
live-provider rehearsal. It does not certify the full release gate. The
[presenter script](#presenter-script-and-stage-cues) below pairs spoken lines
with the actual controls and evidence to show.

The closing reveal links to the verified public attendee kit at
`https://conference.rudeless.ai/kit/`, with a scannable QR. The landing page offers
the experimental source snapshot, checksum, quickstart, connector contract,
reference client and verified GitHub contribution destinations. Open it on a
phone before presenting. The kit labels the local fixture rehearsal separately
from live-provider results; sharing it does not certify the full conference gate.

## Local checks without provider traffic

Use a Parley checkout containing the conference kit. The Vapi SDK snapshot is
included under `vendor/server-sdk-rust`; Rvoip is the pinned sibling dependency.
`scripts/setup-rvoip.sh` creates a separate pinned Rvoip checkout and applies the
conference patch; it does not alter the original `../rvoip` working tree.

```sh
bash scripts/setup-rvoip.sh
node scripts/verify-conference-dependencies.mjs
npm ci
npm run test:conference-client
cargo test --lib --tests --no-default-features --features sms-fake,uctp,sip
cargo build --no-default-features --features sms-fake,uctp,sip
npx playwright test e2e/conference.spec.ts e2e/conference-recovery.spec.ts
```

The full local media gate requires the default build (including native Opus),
Node dependencies, and an installed Playwright browser:

```sh
cargo test --test uctp_conference -- --ignored --test-threads=1 --nocapture
```

The first explicit gate runs Chrome against a loopback SIP/RTP endpoint and a local
Vapi HTTP/WebSocket fixture. It checks distinct audio tones in both directions,
real Chrome microphone denial before server allocation, rejection of an
unconnected browser handoff without interrupting the original audio, and
preservation of the same SIP connection after a successful handoff. It measures
that AI audio stops while browser audio continues. The telephone participant
then hangs up; the complete-scenario gate separately covers Jonathan ending voice.
It uses no external phone numbers or provider credentials. The test is ignored
by the ordinary Cargo run because it requires a browser; `scripts/ci.sh` runs
both gates explicitly after installing the browser. See the evidence document for their
current result. Native Opus compilation requires a C/C++ toolchain and CMake.

If a shared Playwright cache is incomplete, use a new project-local cache by
setting `PLAYWRIGHT_BROWSERS_PATH=var/conference-browsers` for both the install
command and every browser/media test. Select `PARLEY_BROWSER_CHANNEL=chromium`
for the downloaded browser. This leaves existing shared browser installations
alone. A browser that cannot launch is an environment failure, not a passed
application or media check.

The second gate runs the complete story from a fresh temporary database, using
`config/conference-scenario.json`: task → SIP booker/transcript → organizer SMS
and reply → second SIP call → browser audio takeover → owner approval → four
final fixture texts → actual UCTP request reveal. The external worker runs in a
separate process from the server. The planner and providers are local fixtures;
SIP and Chrome audio are real. It saves labeled screenshots in
`test-results/conference-scenario-handoff.png` and
`test-results/conference-scenario-reveal.png`. Run these gates after Playwright
when retaining screenshots, because Playwright clears its output directory.
Each complete-scenario invocation creates isolated state and tears down its
own server and endpoints; this is the reproducible automated rehearsal, not a
live-provider reset procedure.

The Playwright configuration starts a local fixture server on HTTP 18080 and
UCTP 17443. It uses a deterministic planner, fake SMS, and no carrier or Vapi
requests. Locally it uses installed Chrome by default; set `PARLEY_BROWSER_CHANNEL=chromium`
to use the installed Playwright Chromium. CI uses Playwright Chromium. The
test exercises task submission, owner approval, four recipients, and disclosure
of the actual external worker's UCTP request. It saves a stage screenshot under
`test-results/conference-stage.png`.

The SIP integration test uses an actual loopback SIP peer and checks remote
teardown. It is not a PSTN or bidirectional audio test. A separate `cargo check`
with default features checks the Vapi/WebRTC code compiles.

The ordinary Rust suite also runs a real UDP/SIP failure matrix through UCTP:
busy (`486`), unanswered ringing until the actual 30-second activation deadline,
explicit cancellation, and a final answer arriving after cancellation. The peer
observes final-response ACKs, CANCEL for unanswered/canceled calls, and ACK before
BYE for the late answer. Exact invite replay retains one dialog. Each case leaves
terminal connection/session state, no connected or assistant-attached event, and
a Conversation that can accept the next task. This test intentionally adds about
30 seconds to the suite and needs no browser or provider credentials.

## Operator entry points

Run the complete automated rehearsal from the repository root:

```sh
bash scripts/run-conference-demo.sh rehearsal
```

This invokes the complete browser/SIP scenario described above, with a fresh
test-owned database and local provider fixtures on every run. It sends no live
traffic, even when provider credentials exist locally. Install/build prerequisites
first; the command does not silently install dependencies or replace a server.

For a live server, build the default target and supply the intended private
configuration, a private `PARLEY_API_SECRET`, and explicit `PARLEY_SQLITE_PATH`:

```sh
bash scripts/run-conference-demo.sh live-server
```

The launcher selects live mode, preserves the specified database, and disables
automatic provider resource provisioning. Tunneling defaults off; explicitly set
`PARLEY_TUNNEL=1` only for your configured tunnel. Existing queued outbox work in
that database remains eligible; choose the intended rehearsal state. This does
not start the worker, send a test SMS, or dial a test call on its own.

After provisioning, inspect the actual authenticated UCTP profile and local state:

```sh
bash scripts/preflight-conference.sh var/conference/<cid>/provisioned.json local
# Select live for the live server's required voice, SMS, and provisioned routes:
bash scripts/preflight-conference.sh var/conference/<cid>/provisioned.json live
```

The stage and snapshots identify the compiled dependency baseline, patch
fingerprint, experimental profile, envelope version, and WebSocket control
binding. `verify-conference-dependencies.mjs` verifies local source files before
build/launch; it is not an attestation of an arbitrary remote executable. Record
the final server binary checksum and deployment revision at the release freeze.

Preflight reads the owner's private bundle, prints no credentials or phone
numbers, and exits nonzero on a closed Conversation, unsettled work, overlapping
SMS endpoints, missing roster/routes, or unavailable required capabilities. It
makes no external provider probes. A passing result is configuration evidence;
actual provider credentials, callbacks, reachability, and audio still require the
live rehearsal below. Refresh expired participant tokens before running it.

Preparation is resumable. Choose a private state file for each new rehearsal:

```sh
PARLEY_HTTP_URL=https://conference.rudeless.ai \
UCTP_URL=wss://conference.rudeless.ai/uctp \
node scripts/provision-conference.mjs private-contacts.json \
  --state var/conference/preparations/rehearsal-oct12.json
```

Supply `PARLEY_API_SECRET` securely through the environment. This command only
creates the roster and owner/assistant credentials. It saves the exact UCTP
create envelope before sending, then saves the returned identity and each token.
If the connection drops or token issuance fails, repeat the same command and
state path: it replays the saved create request or completes the missing tokens.
It refuses a different roster or server with that state file. Files are private;
credentials and contact endpoints are not printed. If no `--state` is supplied,
the command prints a new preparation path before contacting the server; retain
that path for retries.

Tokens last 12 hours. Repeating preparation refreshes expired tokens while
preserving the Conversation. Add `--refresh-tokens` to refresh them earlier,
then reconnect the stage and restart the worker with the updated bundle.
A preparation lock prevents concurrent runs for the same file. If a process
crashes, verify it has stopped before removing its `.lock` file. A server-side
pending/unknown create result remains an operator investigation; do not change
the request ID or state file to force a second Conversation.

## Close one rehearsal and start another

Use this procedure before every full run from the beginning. Each run has a new
Conversation ID and fresh worker state; within that run, every participant and
handoff shares that one Conversation. Continuing an interrupted run is a separate,
explicit choice and preserves its existing facts and history.

Stop the assistant worker, end voice normally, and wait for SMS submissions to
settle. Resolve interrupted calls and unknown provider outcomes first. Save a
private decision file with a unique command ID and a short completion note:

```json
{
  "request_id": "env_close_rehearsal_oct6_1",
  "verification_note": "Calls ended, SMS submissions settled, and stand-ins informed that this rehearsal is over."
}
```

Close only that bundle's Conversation:

```sh
node scripts/reset-conference.mjs var/conference/<cid>/provisioned.json var/conference/close-decision.json
```

The owner-scoped UCTP command refuses active/interrupted Sessions and queued,
submitting, or unknown SMS. Success preserves the database, journal, delivery
receipts, and worker evidence while disabling new effects and retiring that
Conversation from inbound SMS routing. The stage becomes read-only. If an
assistant was still running, it observes closed state and exits without resuming
saved effects. This command never kills processes or erases a database.

After a timeout, retry the exact decision file. If the server explicitly rejects
it, resolve the reported blocker and save a new decision with a new request ID;
replaying the rejected ID intentionally returns its original rejection.
Provision a fresh Conversation with `scripts/provision-conference.mjs` and a new
preparation state path, use its
new bundle and worker state, and rerun preflight. Do not copy pending worker
commands into the new Conversation or delete old state to force another send.
SMS itself has no Conversation ID: tell stand-ins when the task changes and
avoid late replies to an old rehearsal. Distinct sender numbers are needed when
the same people must participate in overlapping trips without manual routing.

## Provision a local Conversation

Start the server with provider traffic explicitly disabled:

```sh
PARLEY_VAPI_CHAT=fake PARLEY_TUNNEL=0 PARLEY_PROVISION=0 \
  PARLEY_API_SECRET=dev-only PARLEY_BIND_HTTP=127.0.0.1:8080 \
  PARLEY_BIND_UCTP_WS=127.0.0.1:7443 \
  PARLEY_SQLITE_PATH=var/conference-local.sqlite ./target/debug/parley
```

In another terminal, use the local admin secret to provision the fixture roster:

```sh
PARLEY_API_SECRET=dev-only node scripts/provision-conference.mjs \
  examples/conference-assistant/contacts.fixture.json
```

This creates membership and scoped credentials; it sends no messages or calls.
It saves `var/conference/<cid>/provisioned.json` with restricted permissions.
Open `http://127.0.0.1:8080/conference/`, enter the saved Conversation ID, and
paste the owner's token. The assistant token belongs only to the external
worker. The fixture contacts are fictional numbers and must not be used with
live providers.

The browser does not include an embedded AI. For a deterministic walkthrough,
use the Playwright test above. To use Vapi inference with fake messaging, keep
the server in fake mode and launch the worker with `VAPI_PRIVATE_KEY`,
`CONFERENCE_CID`, and `CONFERENCE_ASSISTANT_TOKEN` supplied securely:

```sh
node examples/conference-assistant/worker.mjs
```

That worker launch uses the real Vapi Chat API and may incur provider usage.
Voice is advertised only when the relevant server adapters are configured.
The worker persists its commands and progress under `var/conference/<cid>`.
After a disconnect, restart with the same state. Do not delete pending commands
to force retries. Remove a stale `.lock` only after confirming its worker has
stopped. Scoped participant tokens currently expire after twelve hours; mint
fresh tokens for the rehearsal and performance.

## Live rehearsal prerequisites

To run the complete story with real Vapi planning and voice together:

```sh
# VAPI_PRIVATE_KEY is required; VAPI_ASSISTANT_ID optionally selects the assistant.
PARLEY_BROWSER_CHANNEL=chromium bash scripts/run-conference-demo.sh live-vapi
```

This uses two synthetic local SIP participants and fixture SMS, with real Vapi
usage. It checks the spoken itinerary and pickup facts, browser takeover of the
same organizer connection, owner approval, and four individually addressed final
updates. Both Vapi calls must match the Conversation/Session metadata and end.
The gate requests teardown before reporting a scenario failure. Screenshots use
the `conference-live-vapi-` prefix and explicitly label the provider mix.

On macOS, the launcher generates speech using `say` and `afconvert`. Elsewhere,
set `CONFERENCE_SCENARIO_SPEECH_DIR` to a directory containing `booker.pcm` and
`organizer.pcm`: each 1–45 seconds of raw 8 kHz mono signed 16-bit little-endian
audio. Use the exact role scripts in `scripts/prepare-conference-speech.mjs`.
Set `PLAYWRIGHT_BROWSERS_PATH` if Chromium is installed in a custom directory.
This mode does not exercise PSTN, Telnyx delivery, or the conference network.
The default rehearsal and CI explicitly disable both live planning and voice.

The separate real-voice gate uses the configured Vapi assistant with synthetic
speech on a local SIP endpoint, then joins that same call through the stage UI:

```sh
# VAPI_PRIVATE_KEY is required. VAPI_ASSISTANT_ID can override the local provision cache.
PARLEY_BROWSER_CHANNEL=chromium bash scripts/run-conference-demo.sh live-voice
```

On macOS the launcher uses `say` and `afconvert` to generate the organizer's
speech locally; no recorded person or additional speech provider is needed.
On another platform, set `CONFERENCE_SPEECH_PCM` to 1–45 seconds of raw 8 kHz,
mono, signed 16-bit little-endian synthetic speech saying:

> Yes. This is a sandbox test. Pickup is confirmed at terminal C at five p.m.
> The confirmation word is pineapple. Please repeat the confirmation word.

This mode consumes real Vapi voice usage but sends no PSTN calls or SMS. It
checks that Vapi hears and repeats the confirmation word, that UCTP attributes
both final transcripts correctly, and that audible reply audio reaches SIP.
The browser then takes over with real Opus/G.711 audio in both directions;
Vapi's call resource must confirm retirement while the SIP call remains alive. The browser ends
the call through UCTP. The Conversation must stay open. A labeled screenshot is
saved as `test-results/conference-live-voice-handoff.png`.

The live test lives in its own ignored `conference_live_voice` test binary,
requires `PARLEY_LIVE_VOICE=1`, and is not selected by the offline media gates.
Voice observation is limited to 90 seconds and the browser probe to 60 seconds;
the test requests teardown before reporting a timeout or failed observation.
Its reported `handoff_ui_ms` measures clicking Join through the stage showing
Speaking. `ready_to_bidirectional_audio_ms` measures from the browser's connected
state to both its detection of the SIP tone and the SIP endpoint's detection of
the browser tone. Browser and endpoint clocks are on the same test machine;
the gate requires at most 2,000 ms. This is local media evidence and must be
repeated on the actual conference network.

Provider verification first finds the call under the selected assistant using
the exact Conversation and Session metadata, then polls only that call's REST
resource. It requires `status=ended` and an end timestamp. A WebSocket terminal
event is recorded separately: the live service can close the socket without
sending that event before the adapter's shutdown deadline. Neither provider
IDs nor credentials are printed or sent to the browser probe.

Before involving the stand-ins, test real Vapi planning with the local voice and
SMS fixtures:

```sh
# Supply VAPI_PRIVATE_KEY securely. This mode incurs Vapi Chat usage.
PARLEY_BROWSER_CHANNEL=chromium bash scripts/run-conference-demo.sh live-planner
```

This runs the complete fresh-state browser/SIP scenario with the external
worker's actual Vapi Chat decisions. Telephone peers, voice-provider audio and
transcripts, and SMS remain fixtures; no external calls or texts are submitted.
The test limits planning to 20 decisions and six minutes, checks itinerary facts,
requires owner approval, and verifies four individually addressed final updates.
Its screenshots use the `conference-live-planner-` prefix and label the mixed
mode. It does not prove live Vapi voice, PSTN, Telnyx delivery, or conference NAT.
The default `rehearsal` launcher and CI explicitly disable this live option.

The planner requests a pretty-printed singleton decision array and validates
the complete JSON before the worker prepares any effects. The live Chat API
was observed to omit opening object characters in compact/top-level-object
responses. Missing characters are never reconstructed. A malformed response
stops the worker for diagnosis. The response token allowance accommodates all
four final updates; provider-native communications tools remain disabled.
The conference planner defaults to `gpt-4.1`. The earlier `gpt-4o-mini` selection
failed the live workflow gate (wrong channel, an extra owner call, or no proposal).
Changing `VAPI_CHAT_MODEL` requires repeating this gate. This model setting is
separate from the saved Vapi voice assistant. The worker retains its most recent
provider decision in its private state file for diagnosis, including a decision
that fails validation; that file must not be published with the contribution kit.

For locally invalid decisions, the worker permits up to three model attempts
with validation feedback before submitting any effect. Exhaustion stops the
worker; accepted or pending batches continue through durable retry rather than
replanning. Observed human speech start/stop and final transcript events also
delay planning until speech stops and 1,500 ms of observed quiet has elapsed.
These checks reduce premature decisions on fragmented voice transcripts.

Use a private roster with SMS endpoints only for recipients who personally enrolled
at `https://rudeless.ai/sms` and whose requested task was reviewed. The full flow
requires enrolled owner and organizer SMS routes; companion and booker may have
chat-only final updates. Give the booker and organizer SIP routes to their voice endpoints. The confirmed
booker has PSTN only, so both live calls use Telnyx; a trunk SIP URI does not make
either participant a native SIP user. The four-role private draft roster is
`var/conference/live/contacts.live.draft.json`. Contact routes are configured,
but actual availability, delivery and audio still require the live rehearsal.
Provision that roster into a new Conversation. Never reuse the fixture
numbers in live mode. Vapi voice needs the server's API key and assistant ID;
Telnyx SMS needs its API key, verified sender, and webhook verification key.

### Preferred conference booker: Thelve call-center seat

Have the person playing the booker sign into `https://ccaas.vapi.ai`, enable
their softphone and become available for the selected incoming queue. Jonathan
continues to use Parley's browser interface; Jeff answers his ordinary phone.
Thelve's browser softphone, if used, is a separate WebRTC leg from Jonathan's
later Parley-to-PSTN handoff.

Before replacing the private booker voice route, obtain and qualify Thelve's
actual receiving SIP URI or incoming PSTN number. Verify the selected tenant,
queue, agent assignment, caller trust/authentication, signaling transport,
media reachability and two-way audio. The local Thelve source contains SIP/TLS
and browser-voice support, but this does not prove the deployed conference seat
is callable. Direct SIP may require additional Parley transport/trust settings;
do not infer a SIP destination from the website hostname.

Select the qualified route in a new rehearsal roster and preparation file.
Retain the existing preparation and use the configured booker phone for a PSTN
fallback rehearsal. The assistant still addresses the booker Participant with
the same UCTP `session.invite`; server-side routing determines the destination.
Keep the booker's SMS endpoint associated with the person actually playing that
role. Display Thelve's ringing/answered screen beside the Conversation graph,
then return to Parley. Wait for the organizer invitation to ring and display
**Answer organizer call** before Jonathan joins Jeff's active call.
Label whether the call entered Thelve over direct SIP or through PSTN.

Before switching the live sender or attempting a carrier round trip, run the
separate read-only Telnyx configuration check. It is an operator check, not a
communications interface for the assistant. Save a private JSON file with
`number_id`, `phone_number` (US E.164), `profile_id`, `campaign_id` and
`webhook_url` (the exact public HTTPS SMS callback). Supply `TELNYX_API_KEY` or
`TELNYX_TEST_API_KEY` through the private operator environment, then run:

```sh
node scripts/preflight-telnyx-sms.mjs var/conference/live/sender-config.json
```

This performs six GET requests against Telnyx. It requires an active two-way
sender on the expected messaging profile, a real carrier-approved campaign,
completed `ASSIGNED` linkage, an enabled profile and the exact callback URL.
Known draft placeholders in the submitted workflow or HELP response also fail
the check. US STOP, HELP and START/UNSTOP keyword rules must match the approved
campaign's exact messages and keywords. Pending approval or assignment is a stop condition; buying an active
number does not establish messaging readiness. This command never buys a number,
assigns a campaign, changes the sender, sends a message or tests a call. Output
omits phone numbers, provider IDs, callback URLs, credentials and provider bodies.

A passing configuration check still does not prove recipient consent, runtime
keyword responses, delivery or signed callback/reply routing. Test those with
the consenting stand-ins before the complete rehearsal. Campaign registration
text does not configure branded keyword responses on the messaging profile.
Telnyx documents default STOP/UNSUBSCRIBE blocking and START unblocking across
the entire profile; custom HELP and subscription responses need separate
configuration and an actual test. See [Telnyx keyword behavior](https://developers.telnyx.com/docs/messaging/messages/advanced-opt-in-out)
and [number assignment status](https://support.telnyx.com/en/articles/11072276-10dlc-number-assignment-status).


### Live SMS enrollment and approved message framing

The server requires `PARLEY_SMS_ENROLLMENT_PATH` and `PARLEY_SMS_CAMPAIGN_ID`
for live SMS. The enrollment file is private, readable only by the service and
operators. It is an operator-reviewed record of public-form enrollment, not a
replacement signup method and not proof that the form database was queried.
Record the real source and evidence; never fabricate a form timestamp. Its shape is:

```json
{
  "version": 1,
  "source": "https://rudeless.ai/sms",
  "sender_number": "+14155550000",
  "campaign_id": "the-approved-campaign-id",
  "recipients": [{
    "number": "+14155550101",
    "web_enrollment_confirmed": true,
    "reviewed_at": "2026-10-10T19:00:00Z",
    "evidence": "Describe the recipient's enrollment confirmation and reviewed requested task",
    "revoked": false
  }]
}
```

These example numbers are fixtures. Save real records outside tracked source and
replace the file atomically after reviewing enrollment or withdrawing approval.
Missing, malformed, wrong-campaign, wrong-sender, unreviewed or revoked entries
block live SMS before enqueue and again before provider submission. A changed
review rejects queued work as failed without submitting it. Provider submission
with an ambiguous outcome still requires reconciliation; do not resend blindly.
Telnyx continues to enforce profile-wide STOP blocks. Updating enrollment cannot
clear those blocks. START/UNSTOP restores a previous provider subscription and
cannot create an initial enrollment in Parley.

All coordination questions and approved final SMS use `Rudeless Thelve:` branding
and `Reply STOP to opt out.` Requests stay within the approved customer-care
campaign: requested task/demo progress, choices, clarification, confirmations
and completion, without marketing or consent on another person's behalf. The
worker adds the framing and the server rejects messages without it. Final updates
still require authoritative owner approval. Each of the four human roles gets an
individual update; roles without SMS endpoints get Conversation chat. Chat
acceptance is displayed separately from SMS sent and carrier delivered states.

Start a fresh idle full-mode worker only after both configuration gates pass:

```sh
node scripts/preflight-telnyx-sms.mjs var/conference/live/sender-config.json
bash scripts/preflight-conference.sh var/conference/<cid>/provisioned.json live
node scripts/start-conference-worker.mjs var/conference/<cid>/provisioned.json full var/conference/live/sender-config.json
```

The worker launcher rechecks carrier assignment, exact keyword configuration,
server-reviewed SMS eligibility and distinct recipient routes. It receives only
its scoped Conversation credential; carrier and host credentials stay with the
operator/server. Connect the owner page and select **Start coordinating** when
participants are ready. Preparation and an idle worker place no calls or texts.

The SMS webhook acknowledges provider-classified keyword messages separately
from task replies. It reads `autoresponse_type` from the original payload only
after signature verification, because the pinned Telnyx SDK does not preserve
that field. Reserved whole-message keywords are also excluded when classification
is absent. These controls do not become task facts or provoke an AI reply.
Ordinary replies such as `YES` without provider classification, or “help with
pickup,” retain the existing Conversation routing and deduplication. This guard
does not send keyword responses, implement a subscription database or assert
that a carrier block succeeded. Test the real provider behavior separately.

`CONFERENCE_SIP_BIND` enables the dedicated outbound core SIP adapter;
`CONFERENCE_SIP_FROM` supplies its From URI. These are separate from the legacy
SIP listener. The following settings now map to the conference adapters:

| Setting | Purpose |
| --- | --- |
| `CONFERENCE_SIP_USERNAME`, `CONFERENCE_SIP_PASSWORD`, optional `CONFERENCE_SIP_REALM` | Outbound SIP digest credentials; supply username/password together. |
| `CONFERENCE_SIP_ASSERTED_IDENTITY` | Optional P-Asserted-Identity accepted by your trunk. |
| `CONFERENCE_SIP_ADVERTISE` | Reachable SIP signaling IP and port. |
| `CONFERENCE_SIP_MEDIA_PUBLIC` | Public RTP IP and port; port `0` retains the actual allocated RTP port. |
| `CONFERENCE_SIP_MEDIA_PORTS` | Inclusive RTP/RTCP range, default `44000-44200`. |
| `CONFERENCE_WEBRTC_UDP` | Server media bind IP and port, default loopback `127.0.0.1:0`. |
| `CONFERENCE_WEBRTC_PORTS` | Optional inclusive per-peer UDP allocation range. |
| `CONFERENCE_WEBRTC_PUBLIC_IPS` | One static NAT IP matching the bind address family; the current Rvoip driver supports a single advertised address. |
| `CONFERENCE_SERVER_ICE_JSON` | Server-side STUN/TURN entries. |
| `CONFERENCE_BROWSER_ICE_JSON` | Separate browser-facing STUN/TURN entries delivered with the owner's browser offer. |

ICE arrays use objects with `urls`, optional `username`, and optional `credential`.
TURN requires explicit username and credential. Provision browser credentials
specifically for client distribution; server credentials are never implicitly
copied to the browser. TURN credentials are omitted from the stage inspector's
projection, while the authenticated command replay preserves the original offer.
Network settings validate at startup. Configuring a public IP does not create a
NAT mapping or open a firewall; verify the selected ports and actual audio on the
conference network. Live trunk authentication, NAT, and TURN remain rehearsal
gates, not results established by local tests.

Configure and verify HTTPS/WSS and signed Telnyx callbacks. Confirm capabilities
from a UCTP snapshot, then run the eight acceptance steps in
`CONFERENCE_DEMO_PLAN.md`. Log the pinned build, sanitized UCTP evidence, provider
IDs, and audible confirmation in both directions. Specifically verify that
Jonathan joining from WebRTC preserves the organizer's existing SIP dialog.

## Real PSTN/browser listening test

The opt-in test places a real call to the consenting telephone holder in a
private JSON configuration. It sends no SMS and creates a fresh three-role test
Conversation. Use it when the holder is ready to answer; it is not the complete
four-person scenario. The server's existing trunk and Vapi voice assistant must
already be configured. This release uses multiplexed RTCP; configure the
dedicated Telnyx trunk's RTCP port as `rtcp-mux` to match it. The pinned Rvoip
patch starts reports after late SDP and the conference endpoint requires SDP
multiplexing agreement. Confirm `a=rtcp-mux` in the carrier answer; its portal
setting alone did not prove negotiation. Include a hold beyond 60 seconds in live
qualification; short audio samples did not expose the original carrier timeout.

```sh
node scripts/prepare-conference-speech.mjs pstn
# Private contact file: E.164 `phone` plus `speechPcm` path.
# Optional distinct `callback_phone` enables browser-to-telephone movement.
# Optional `hold_after_browser_ms` (0–90000) checks extended media liveness.
# Use the approved recipient and var/conference/pstn-speech/browser.pcm.
# Supply PARLEY_API_SECRET, VAPI_PRIVATE_KEY and VAPI_ASSISTANT_ID privately.
bash scripts/run-conference-demo.sh live-pstn var/conference/live/pstn-test-config.json
```

Vapi asks the telephone holder to say **blue umbrella**. The harness waits for
that person's attributed transcript before allocating the browser. A voicemail,
ended call or readiness timeout stops the test and requests teardown. The real
browser takes over through the stage UI and sends the synthetic phrase **orange
bicycle**; the telephone holder repeats it and describes audio quality. The
browser captures decoded return audio privately. No hardware microphone is
used by this automated probe.

The generated microphone must remain live between utterances: the harness keeps
a zero-valued audio source connected after the speech buffer ends. An ended
buffer without a continuing source stopped browser RTP and invalidated the
previous quiet-period test. Continuous silence is distinct from lost browser
media; qualifying the latter requires a separate source-loss test.

The automated measurements assert encrypted Opus, public AWS UDP, traffic in
both directions, decoded return audio, the retained original Connection and
Session, and provider termination before the telephone Session ends. Final Vapi
status can lag actual termination, so its final timestamps are checked after
teardown. Human listening stays unverified until the actual holder confirms
hearing the browser phrase and their voice in the return recording. Preserve
that confirmation alongside the result; packet counts alone cannot grant it.
Private evidence is saved under `var/conference/live/pstn-*`, including the WAV,
correlated commands, journal, measurements and provider call record. Review
recordings and transcripts before sharing.

For the two-owned-phone test, provision `phone` as the remote phone and
`callback_phone` as Jonathan's callback. After recording browser return audio,
the harness selects **Move to my phone**. Answer the second phone and press
**1** within 45 seconds. The first phone remains on its original call. After the
callback answers, a synthetic voice asks the holder to press 1 and repeats while
waiting. Actual negotiated G.711 silence fills the gaps to keep RTP flowing.
The server stops the prompt before replacing the browser's speaking route.
Answering or hearing the prompt alone never commits a move. After the
committed move, speak **green lantern** on one phone and **silver mountain** on
the other during the 65-second live listening window. Keep both calls open until
the harness ends them; both telephone edges must remain active past the old
60-second timeout. The harness verifies identity,
remote retention and teardown, but only the phone holders can confirm audible
speech in both directions. Preserve their confirmation separately.

To exercise just the spoken browser/capture harness against the synthetic AWS
SIP peer before calling a human, use
`node e2e/run-conference-cloud-media.mjs --speech`. See
`infra/conference/README.md` for the host-local peer prerequisite. That check is
explicitly synthetic SIP and cannot pass the PSTN listening gate.

## Stage sequence

After connecting, **Room view** fits the network diagram and controls to a wide
presentation viewport; evidence panels scroll independently. **Scroll view**
restores the full page. The graph distinguishes UCTP control, WebRTC, SIP, RTP,
Vapi audio, and SMS. Selecting a timeline event or graph edge shows the journal
state at that event and opens its evidence. **Back to timeline** returns to live
connections. The graph reports committed bridge state; audible confirmation
still comes from the actual call.


1. Deliver the opening promise from the plan, then submit Jonathan's travel task.
2. Let the assistant call the booker, gather an alternative, and end that Session.
3. Show the organizer's SMS, attributed reply, and subsequent telephone call.
4. Join the existing call from Jonathan's browser and confirm two-way audio.
   Say “This Wi-Fi is breaking up—move this conversation to my phone.” Select
   **Move to my phone**, answer the callback and press **1**. Confirm both
   telephone participants hear each other and show the retained remote
   Connection and unchanged Conversation/Session IDs.
5. End voice, approve the sandbox proposal, and show four individually addressed
   SMS outcomes under the same Conversation ID.
6. Select the organizer's message in the timeline to reveal the actual UCTP
   request and correlated result. Explain Rvoip's connector role.
7. Expand “Where this goes next.” Invite people to contribute a connector or a
   UCTP application and share it. Use `CONTRIBUTING_CONNECTORS.md` as the kit.

When an outcome is `unknown`, inspect and reconcile the provider state before
retrying. When a network path fails on stage, show the actual failure and use a
clearly labeled prerecorded rehearsal. Do not represent fixture events or
signaling-only connections as a successful live call.

Before publishing the closing QR code, verify the public quickstart and sharing
destination. Keep contacts, credentials, provisioning bundles, and raw private
traces outside the contribution kit. The public kit destination is verified. Full live provider/media validation
and the final kit refresh are still outstanding.

## Presenter script and stage cues

Allow eight minutes for the story and another 60–90 seconds for the interface
reveal and closing. This is a rehearsal budget, not a confirmed conference slot.
Keep the configuration and credentials off the projector; connect the stage
before the opening and select **Room view**. Have Jonathan's callback phone
ready. The booker and organizer need the agreed sandbox facts and must know
when to answer and reply. Alex receives the final update. Use the real private
roster only for an agreed live rehearsal; the automated fixture rehearsal uses
its own local participants.

### 0:00–0:45 — The promise

Say:

> What if your AI could reach the people it needs through the ways they already
> communicate—and keep the whole task in one Conversation?
>
> My flight has been canceled. I’m going to give David, my AI assistant, one goal and one
> communications interface. You’ll watch it call my travel booker, text and
> call the conference organizer, bring me into that same call from my browser,
> then move me to my phone. Finally, it will text everyone the agreed plan.
> We’ll watch the context build across these networks, and then I’ll show you
> the interface the assistant actually used.
>
> The travel arrangements are a sandbox scenario. In this live version, the
> telephone calls and text messages are real.

Use that last sentence only in a qualified live run. For the local rehearsal,
say instead: “This is a local rehearsal with simulated providers and real local
browser/SIP media.” A recording must retain its visible replay/fixture label.

### 0:45–2:15 — Give the AI a goal; recruit the booker

Read the task already in the composer:

> My flight was canceled. Help Alex and me arrange an alternative with the
> travel booker, update the conference organizer, and bring me into the
> confirmation call. Ask me to approve the final sandbox arrangements before
> texting everyone.

Select **Start coordinating**. Let the assistant and booker talk. The booker's
sandbox option is two seats on **RD742**, departing **16:00**, arriving at
**terminal C at 17:00**; no real purchase has been made. Speak naturally rather
than reading a long response. The assistant should gather the facts and end
that voice Session before starting the organizer's call.

Say while pointing at the stage:

> The booker uses an ordinary phone. Rvoip reaches that phone through the SIP
> trunk. The assistant learns something useful, and that context stays with
> this Conversation after the call ends.

If the verified Thelve seat is selected, describe the actual receiving route
instead: its call-center browser endpoint is a separate application and media
leg. A browser login alone is not evidence of a direct SIP endpoint.

### 2:15–3:45 — Text the organizer, then call to confirm

Show the organizer coordination SMS event and its actual delivery state. The
organizer replies: **“Terminal C at 17:00 works. Please call to confirm the
pickup.”** Show the attributed inbound reply under the same Conversation.
The assistant then calls the organizer, who confirms the pickup and asks to
bring Jonathan into the call.

Say:

> We’ve changed medium, but we haven’t started over. The reply belongs to the
> organizer in this same Conversation. Now the assistant calls to check the
> final arrangements.

Do not call an SMS “delivered” until its delivery receipt says so. Submission,
delivery, and the organizer's reply are separate events. If a reply cannot be
attributed, resolve it through the operator inbox; do not pretend the AI got it.

### 3:45–4:45 — Join the existing telephone call from the browser

Select **Join the telephone call** and allow the microphone. Wait for the
committed speaking route. Say through the browser:

> Hi Jeff. Can you hear me? Alex and I will arrive at terminal C at five.
> Is that pickup still good?

The organizer answers through their original telephone call. Confirm that each
side actually heard the other, then point to the WebRTC and SIP/PSTN edges.

> My browser has joined the existing call. Jeff still has the same phone call;
> this is the same voice Session and the same Conversation.

For a stand-in rehearsal, use that person's name. Jonathan replaces the AI's
speaking media leg here; the AI keeps its control connection. This demo does
not require or claim three simultaneous speakers or a live whisper mixer.

### 4:45–5:45 — Move to the phone without starting over

Say:

> Suppose I need to leave the laptop. I want to continue this conversation on
> my phone.

Select **Move to my phone**. Answer Jonathan's callback. Let the audience hear
the synthetic prompt: **“To move your existing conversation to this phone,
press one now.”** Press **1**. Keep the organizer's original call open.
Continue through the phone:

> Jeff, I’m on my phone now. Can you still hear me?

After the answer, show the confirmed phone speaking route and the retired
browser route, with the same Conversation, Session and Jonathan Participant.

> The endpoint changed. The person, the context, and Jeff’s original call
> stayed together. This is the VoIP 3.0 experience we’re building.

This is an explicit user-confirmed move, not a demonstration of automatic Wi-Fi
failure detection, QUIC connection migration, or measured gap-free audio. If
using “bad reception” as the story, introduce it as a scenario instead of
claiming the venue network failed. While waiting, the browser remains connected;
if the callback fails, use **Cancel phone move** and continue from the browser.

### 5:45–7:15 — Approve once; update everyone

Confirm the pickup, say goodbye, then select **End voice** and wait for terminal
call state. Read the visible proposal; it must contain the agreed flight,
arrival, terminal and pickup facts, with the sandbox label. Select
**Approve these sandbox arrangements** only when it is correct.

Say:

> I approve the plan. Now the assistant can send the final arrangements to me,
> Alex, the travel booker, and Jeff. Four individually addressed texts, one
> Conversation.

Show the four recipient outcomes. Recipients can hold up their phones when
messages actually arrive. Individual SMS messages are not a shared carrier
SMS group: the shared Conversation, membership, history and context are held
by the application and exposed through UCTP.

### 7:15–8:00 — Reveal the actual interface

Select the organizer's message in the timeline. Show the actual captured UCTP
request and correlated result. Point out the explicit application profile,
request ID, Conversation ID and recipients. Do not type a staged JSON example
in place of the request that actually performed the action.

Say:

> Here is the request the assistant sent. Its communications actions use this
> experimental UCTP interface. Rvoip and the host’s connectors handle the
> different services underneath it.
>
> A provider such as Telnyx can supply calls and texts. The reason for this
> interface is to give agents and human applications a shared contract across
> connectors, with identity, context, outcomes and capabilities carried through
> the Conversation. Every application should not have to rebuild that
> integration separately for every service it wants to reach.

The binding shown today is **UCTP over secure WebSocket**. If mentioning QUIC:

> UCTP separates the application interface from its transport. Rvoip also has
> QUIC transport support. Today’s browser demo uses secure WebSocket for control,
> WebRTC for browser audio, and SIP/RTP for the telephone leg.

Do not imply this run used QUIC or proves a QUIC-specific performance benefit.

### 8:00–9:30 — The open-source invitation

Expand **Where this goes next** and keep future connectors visibly labeled.

Say:

> The bigger opportunity is the connector ecosystem. We want the open-source
> community to connect more of the disparate ways people communicate. Agents
> and human applications come through UCTP, and Rvoip connects them to the
> systems and media those people already use.
>
> Bring a system you want to connect, or an agent you want to give a voice.
> Build a connector and share it. Build a UCTP application and show us what it
> can do. Help us improve the protocol through real use.

Show the QR for **https://conference.rudeless.ai/kit/**. It offers the source
snapshot, quickstart, connector guide and verified contribution destinations.
Present today’s interface as experimental, and future coverage as the direction
participants can help build.

### Operator stop points and fallback

If a live step fails, show its real state. Say: “This path hasn’t completed.
I’ll show you the same flow from our labeled rehearsal.” Switch to the checked,
audible backup once it is recorded and qualified. The existing labeled visual
fixture walkthrough is not yet a final live-provider audio backup.

A timeout or `unknown` provider result is not permission to dial or send again:
inspect and reconcile it first. Leave the worker paused and avoid overlapping
calls. Do not proceed to the four final texts without the owner’s approval.

## Current readiness checklist — October 6

| Requirement | Evidence / remaining work |
| --- | --- |
| External AI planner and voice, UCTP request reveal, shared context | Combined real Vapi/local SIP/browser story passes; SMS is a fixture in that run. |
| Browser joins actual PSTN call; telephone endpoint move | Live audio, DTMF-confirmed move and the 65-second post-join hold pass; the user confirmed clear speech in both phone directions. |
| SMS to all four people and organizer reply | Carrier campaign approval/linkage and real delivery/reply checks remain. Last carrier check: `TCR_ACCEPTED`, unassigned. |
| Preferred Thelve booker seat | Receiving route and live seat/media unqualified; booker's PSTN phone remains fallback. |
| Whole story with all live providers and four people | Still needs complete fresh-state rehearsals; component tests do not satisfy this. |
| Operator reset, room audio and venue connectivity | Local/scoped reset checks pass; full live reset and conference network/audio rehearsal remain. |
| Backup and attendee release | Existing visual fixture recording and public v2 kit are verified. Record final audible backup and refresh/freeze kit with phone move. |

See [implementation evidence](CONFERENCE_IMPLEMENTATION_STATUS.md) for detailed
results and the [demo plan](../CONFERENCE_DEMO_PLAN.md) for acceptance gates.
No calls or messages are placed by reading this script.

## Record a labeled visual walkthrough

The complete local rehearsal can record the real stage view with pauses at the
handoff, proposal approval, interface reveal, and community invitation:

```sh
# Install the matching browser and recorder once, using Node 22 or 24.
PLAYWRIGHT_SKIP_BROWSER_GC=1 PLAYWRIGHT_BROWSERS_PATH=var/conference-browsers \
  npx playwright install chromium ffmpeg --no-shell
PLAYWRIGHT_BROWSERS_PATH=var/conference-browsers PARLEY_BROWSER_CHANNEL=chromium \
  bash scripts/run-conference-demo.sh record-rehearsal
```

The launcher forces fixture planning, voice-provider behavior and SMS. SIP and
browser media still run through the actual adapters. The visible label says
**RECORDED REHEARSAL · SILENT VIDEO** and identifies the provider mix. This
recording is intended for presenter narration: Playwright captures no audio,
and it cannot substitute for an audible recording of the final live rehearsal.
It must never be presented as a live PSTN call or delivered Telnyx messages.

Successful browser scenarios save
`test-results/recordings/<mode-prefix>-<cid>/rehearsal-silent.webm`, with a
`recording.json` sidecar containing the video and corresponding evidence-file
SHA-256 hashes. Check that the outer Rust gate also passes; it verifies SIP and,
for live Vapi mode, provider teardown after the browser process exits. Failed
recordings can remain as diagnostic files but receive no success sidecar.

Set `CONFERENCE_RECORD=1` with `live-vapi` to capture the combined real Vapi
scenario instead. It still uses synthetic SIP participants, fixture SMS and a
silent video; Vapi usage applies. Record the final performance separately with
the consenting stand-ins and actual room/remote audio after its live gates pass.

The installation above uses the project-local browser cache. On this machine,
Node 26.3 stalled during Playwright archive extraction; the bundled Node 24
runtime completed installation. No global browser or system FFmpeg repair is
required for the recorder.

## Save the interface evidence

After a settled rehearsal, export its journal and the commands referenced by
that journal through the same read-only UCTP interface:

```sh
node scripts/export-conference-evidence.mjs \
  var/conference/<cid>/provisioned.json \
  var/conference/<cid>/evidence.json live-providers
```

Use one explicit mode: `local-fixture`, `live-planner-fixture-voice-sms`,
`live-vapi-fixture-sms`, or `live-providers`. The exporter checks the declared
SMS mode against the server; the remaining mode description is an operator
declaration, not independent proof of provider delivery or working audio.
It does not send messages, place calls, or modify the Conversation.

The file contains the implementation banner, participant IDs/names/roles,
ordered journal events, actual requests and correlated replies, and the event
sequence numbers associated with each request. Missing or ambiguous command
records are listed explicitly. Commands with no journal event, including rejected
requests, are outside this export. A pending command keeps a null response.
Collection drains bounded journal pages but is not an atomic database snapshot;
finish the scenario first and retain the accompanying gate results.

Credential and SDP-secret fields receive inspector redaction, and participant
route fields are omitted. Message bodies, voice transcripts and other event
content remain. Files are created with mode 0600, and an existing file or symlink
is never overwritten. Review personal content and endpoints before sharing;
this file is private evidence, not an automatically public trace.

The automated complete-scenario gate also saves a uniquely named file under
`test-results/<mode-prefix>-<cid>-evidence.json`, with browser audio measurements.
It checks that every assistant action has an exported correlated reply and that
the owner/assistant tokens and configured Vapi key are absent. These files and
the screenshots are ignored by Git. They support the wire reveal; they are not
a backup recording or a release certificate.


## Recover an interrupted rehearsal

On server startup, an SMS submission interrupted while awaiting its provider
result becomes `unknown` in both the outbox and Conversation journal. It is never
automatically requeued. Queued messages that were not submitted remain eligible
for sending. Check the provider result before any manual resend; a missing
receipt does not prove the message was not sent.

A persisted voice Session becomes `interrupted`: the new process cannot prove
that the remote call ended. The same Session and Connection IDs remain in the
journal, and another invitation is blocked. The external worker pauses planning
and saved effects. The stage view disables join/end and shows a verification
control. Check the remote phone or carrier/Vapi console, terminate any surviving
call there, then enter how you verified termination and select **I verified the
remote call has ended**. This records an owner-attributed fact and permits the
worker to continue. It does not send a hangup to an orphaned provider call.

The corresponding owner-only command is `session.update` with `kind:
confirm_ended`, the interrupted `sid`, and a nonempty `verification_note`.
Assistant credentials cannot issue this attestation. Replaying the same command
returns the same result without a second event. Ordinary Session end does not
bypass verification for interrupted calls.


## Resolve an ambiguous SMS reply

If a sender/local-number pair belongs to multiple open conference Conversations,
the server saves the reply in a private holding inbox and returns a successful
webhook acknowledgement. It does not forward the text to any candidate. Closing
one candidate does not automatically release a previously held reply.

Use the administrator utility from a private terminal, with `PARLEY_API_SECRET`
and optional `UCTP_URL` supplied securely:

```sh
node scripts/conference-inbox.mjs list
```

This prints held text and the original candidate Conversation/participant IDs.
Do not project or share this private output. Verify the intended trip with the
sender, then save a private resolution file such as
`var/conference/resolution.json`:

```json
{
  "request_id": "env_resolve_pickup_1",
  "inbox_id": 1,
  "conversation_id": "conv_selected_candidate",
  "participant_id": "part_selected_candidate",
  "verification_note": "Organizer confirmed this message concerns Jonathan's current trip."
}
```

Replace the example IDs with the actual inbox entry and candidate IDs. Use a
unique request ID for each decision, then retain the exact file for replay:

```sh
node scripts/conference-inbox.mjs resolve var/conference/resolution.json
```

Both commands use UCTP. Owner and assistant tokens cannot administer the inbox.
Resolution creates one attributed inbound SMS with an administrator routing
annotation, then removes the entry from the held list. It sends no outbound SMS.
If the response is lost, rerun the same file; do not change its decision or ID.
The selected Conversation must still be open and its participant endpoint must
still match the sender. No candidate is selected by recency or position.

Before a live rehearsal, check that previous demo Conversations do not leave
multiple reply routes open for the same four contacts. Voice ending alone does
not close a Conversation, because subsequent SMS replies must retain context.

## Observation and history recovery

The stage view and external worker poll journal snapshots using their last
observed cursor. Each page contains at most 500 authorized events. History
responses are also bounded to 500 authorized messages; the JS helper drains
pages using fresh request IDs. Cursors can skip values belonging to other
Conversations or messages the participant cannot see.

Custom clients may request a live subscription. Observe its returned expiry:
leases last at most 30 seconds, one per peer/Conversation, with at most 64 live
observers per host. A duplicate active subscription returns 409 and capacity
exhaustion returns 429; cursor snapshots continue to work. When a lease expires
or a transport stalls, recover from the last event actually received. A dropped
end notice does not extend a lease or erase journal facts.
