# Conference host

The October 2026 demo uses a dedicated CloudFormation stack, VPC, instance and
static IP in Vapi Internal (`vapi-admin`, `us-west-2`). Administration uses AWS
Systems Manager; there is no SSH ingress. The public application is
`https://conference.rudeless.ai/conference/` and UCTP uses
`wss://conference.rudeless.ai/uctp`.

`cloudformation.json` provisions the host and private source-artifact bucket.
`install-host.sh` verifies the source archive SHA-256, builds the pinned Rust
application as an unprivileged user, installs systemd services and configures
Caddy HTTPS/WSS. The archive must include the actual local Rvoip patches, vendored
Vapi SDK, Cargo lockfile, declared test targets and runtime static assets. Do not
package local state, credentials, Git metadata, recordings or provider evidence.

Runtime configuration is one encrypted SSM parameter,
`/parley/conference-2026/runtime`. The host writes it to a root-only environment
file; credentials must not appear in Run Command arguments, source bundles or
logs. An explicit IAM deny prevents the instance role from reading unrelated
SSM parameters; simulation verifies the runtime parameter is allowed and an
unrelated parameter is denied. Both provider provisioning and the older Cloudflare tunnel bootstrap stay
disabled on this host. The existing `parley.rudeless.ai` route is unchanged.

The edge exposes the conference assets, public attendee kit, authenticated conference token endpoint,
UCTP, health check and signed Telnyx SMS callback only. Legacy public visitor
token issuance and other application surfaces are not exposed. SIP UDP 5062 is
restricted to Telnyx US signaling addresses; RTP UDP 44000–44200 is restricted to
Telnyx's published media ranges. WebRTC DTLS media uses UDP 46000–46100. Refresh
Telnyx addresses from `https://sip.telnyx.com/voice.json` before future deployments.

## Attendee kit publication

Build a new source-only snapshot with
`python3 scripts/package-conference-kit.py <new-private-output-directory>`.
Supply configured secrets through the environment for the known-credential
check. The packager uses a source allowlist, rejects symlinks/unexpected files,
excludes runtime state and recordings, and includes individual source hashes,
the base commit and the archive checksum. It represents the current working tree,
not an assertion that all conference changes are already on the public branch.
Test the extracted archive, including the complete local media rehearsal, before
publishing. `tests/test_conference_kit.py` verifies archive boundaries and checksums.

Upload a tar.gz containing only the flat public output files to the existing
private artifact bucket. Run `publish-kit-host.sh <bucket> <key> <sha256>` through
SSM on the host. The script verifies the artifact and embedded archive checksum,
restricts the public file list, validates Caddy configuration, then selects an
immutable static directory and reloads Caddy. A reload or health failure restores
the previous configuration. It does not rebuild or restart Parley. `/kit/` is
a static public surface; administration and participant tokens remain separate.

Verify the actual HTTPS destination using
`node e2e/conference-kit.mjs https://conference.rudeless.ai/kit/ <private-evidence-directory>`.
Check the saved desktop/mobile screenshots. Only after public verification,
generate a QR using `create-conference-qr.py` with Python `qrcode==8.2` and Pillow,
then independently decode it. The macOS Vision example is
`swift e2e/decode-conference-qr.swift <join-qr.png> https://conference.rudeless.ai/kit/`.
Include the generated PNG/SVG in the next static kit publication. Refresh the
snapshot and evidence at the final release freeze; a working attendee kit does
not certify the complete live-provider release gate.

For a stage HTML-only update, upload the tested `web/conference/index.html` and
run `publish-stage-host.sh <bucket> <key> <html-sha256>` through SSM. It retains
the previous HTML, atomically replaces the served file, checks the public file's
exact hash, and restores the previous version if verification fails. Record that
static checksum separately from the running binary and its build-source archive;
this command does not rebuild or restart Parley.

## Application updates

Run `update-host.sh <bucket> <source-key> <source-sha256> <unique-release>` through
SSM for an already-installed host. The private archive contains regular source
files under `parley/` and `rvoip-conference/`, including the pinned patch set,
vendored SDK, lockfile, declared test targets and static assets. Verify the
dependency fingerprints and source archive before uploading. Choose an idle
rehearsal interval: the script builds the new ARM64 release while the current
service runs, then refuses activation if the database has active voice or
unsettled SMS. It takes a private SQLite backup after stopping the service,
selects the new release and checks local health. Activation failure restores the
previous symlink and restarts that release. Runtime secrets, sender settings,
Caddy configuration, database path and public kit remain unchanged. A restart
disconnects idle UCTP clients; reconnect them for the next rehearsal.

Use a fresh SSM command ID and retain its result. A running command or observation
timeout is not a failed build; inspect that same command and its build log before
retrying. New release directories are immutable candidates; the updater refuses
to overwrite one. Failed builds leave the current service running.

After success, record the deployed binary/source hashes and verify public HTTPS
and authenticated UCTP using an existing private fixture:

```sh
node e2e/conference-deployment-smoke.mjs <existing-private-fixture.json> <new-private-result.json>
```

This checks health/static content, public token protections, unsigned-SMS
rejection and scoped reading of the existing Conversation. It sends no calls or
texts, creates no Conversation and changes no task state. It is a deployment
smoke check, not proof of the complete live conference scenario.

The initial build/runtime instance is `m7g.xlarge` with an encrypted 80 GiB gp3
volume. The AWS price lookup on October 6 returned $0.1632/hour for Linux in
Oregon, excluding storage, IPv4 and traffic. The review-date resource tag is
informational; it does not stop or delete resources automatically. The S3 bucket
is retained on stack deletion and build objects expire after 30 days. The SSM
parameter and Cloudflare DNS record are managed separately. Preserve needed
private evidence before any later cleanup.

Infrastructure creation, an application health check and SIP call setup are
separate acceptance gates. Only real transcripts/audio and provider receipts
establish live voice and SMS success. See the implementation status for actual
results; an eligible SMS brand/campaign is still required.


## Public WebRTC media gate

`examples/conference_media_peer.rs` is an explicitly synthetic SIP endpoint:
it listens on AWS loopback port 5094, emits 660 Hz audio and measures received
880 Hz audio. Build this example on the ARM64 host with the same pinned source
and dependency tree. It never dials a recipient. The production binary does not
need to be rebuilt or restarted.

The synthetic peer must also require RTCP multiplexing so its SDP answer
matches the conference endpoint's single RTP/RTCP socket policy. Rebuild the
example after changing that policy; an older peer can fail SDP negotiation
before Vapi attaches.
The example permits six minutes of active call time, and the transient unit
has a seven-minute guard. This leaves room for the AI hold, remote status
queries and browser takeover. Normal cleanup ends the peer as soon as the
gate finishes. These are test resource limits; carrier timeout qualification
uses the carrier's actual events.

With the deployment environment loaded privately and `vapi-admin` authenticated,
run `node e2e/run-conference-cloud-media.mjs` from the project root. This opt-in
gate uses real Vapi (billable), a fresh Conversation with no phone/SMS contacts,
and isolated Chromium on the operator's Mac. It loads the public HTTPS stage,
uses UCTP over WSS and asserts a selected UDP candidate at the AWS public IP,
connected DTLS, Opus and decoded tones in both directions. It also checks Vapi
retirement, retained SIP Connection identity and teardown. Vapi REST status can
lag termination by more than 30 seconds, so the gate confirms final provider
timestamps after ending the test: the Vapi leg must have ended before the
retained SIP Session. It does not equate immediate REST status with media state. Browser microphone
audio is generated; no hardware microphone access or PSTN/SMS sends are needed.

The runner uses a temporary host-local route for the host's own Elastic IP so
the synthetic SIP leg can return RTP without an EC2 NAT hairpin. It preserves
an existing route, removes a route it created, and stops the bounded transient
peer service on exit. A private cleanup record is saved before the test; after
an abrupt process interruption, inspect that record and the named SSM commands
before cleanup or retry. No firewall ports are added. The test is specific to
this deployment; review its host/resource constants before using another stack.

Evidence and screenshots are saved privately under `var/conference/live/webrtc-*`.
This gate verifies the Mac-to-AWS WebRTC path and the SIP bridge, not PSTN audio,
a physical microphone/speaker listening test, or a conference venue network.

### RTCP qualification

The conference RTP Session currently multiplexes RTP and RTCP on one socket.
The conference SIP configuration requires SDP multiplexing negotiation; peers
that decline it are rejected before media commit. A separate-port SIP peer is
not supported by this conference configuration.
The dedicated Telnyx connection must use RTCP port `rtcp-mux`; `rtp+1` does not
match this transport. Keep the carrier report frequency at 5 seconds. The
conference Rvoip patch starts periodic reports even when the SIP peer is learned
after RTP Session creation and refreshes the current report destination after
SDP changes. Qualify actual outgoing RTCP and a call held beyond 60 seconds before
accepting a release. `RTP-RTCP Timeout` in a carrier BYE is a failed media gate.
The standalone patches are `patches/rvoip/rtcp-late-peer.patch` and
`patches/rvoip/rtcp-mux-negotiation.patch`. Require `a=rtcp-mux` in the carrier
answer; changing its connection setting alone did not prove SDP agreement.
