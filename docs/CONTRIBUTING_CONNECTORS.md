# Bring the next connection

Rvoip connects existing systems. UCTP offers a common Conversation interface to
human applications and agents. We invite conference participants to build a
connector, build a client, or improve the contract through a reproducible example.

Start with the [conference profile](UCTP_CONFERENCE_PROFILE.md), the
[implementation evidence](CONFERENCE_IMPLEMENTATION_STATUS.md), and the
[local runbook](CONFERENCE_RUNBOOK.md). The profile is experimental. It currently
runs over WebSocket; broad transport and connector coverage is the direction.

## Try the three actions from the talk

The same authenticated client sends a message, starts voice, and joins the
existing voice interaction. In the browser, with a provisioned owner `token`,
Conversation `cid`, and the server's `url`:

```js
import { UctpClient } from '/uctp-client/client.mjs';
import { BrowserAudio } from '/uctp-client/browser-audio.mjs';

const client = new UctpClient(url, token);
await client.connect();
const snapshot = await client.snapshot(cid);
const organizer = snapshot.participants.find(p => p.role === 'organizer');

// 1. Address a person; the server resolves the SMS connector and endpoint.
const update = client.command('message.send', cid, {
  msg_id: `msg_${crypto.randomUUID().replaceAll('-', '')}`,
  to: [organizer.participant_id], delivery: 'sms',
  content_type: 'text/plain', body: 'Can we confirm the revised pickup?',
});
await client.request(update); // retain `update` for exact retry if its response is lost.
```

Start voice through the same participant's provisioned route:

```js
// 2. Start voice through that person's provisioned route.
const invitation = client.command('session.invite', cid, {
  to: organizer.participant_id, medium: 'voice', purpose: 'Confirm revised pickup',
});
const accepted = await client.request(invitation);
const sid = accepted.payload.session.sid;
```

Wait for the journal's `session.assistant_attached` event for this `sid`, then run
the join action from a browser user gesture:

```js
// 3. Once the journal reports session.assistant_attached, the owner can join.
// Run this from a user gesture so the browser can request microphone access.
const audio = document.createElement('audio');
audio.autoplay = true; document.body.append(audio);
const voice = new BrowserAudio(client, cid, audio);
await voice.join(sid);
```

These are real operations. Use the local fixture rehearsal first; live mode
contacts the provisioned person. An accepted invitation has not yet established
audio. Observe journal outcomes before joining, and keep the same request ID
when retrying an operation whose response was lost. `BrowserAudio` negotiates
media and performs the final UCTP handoff; it keeps the telephone connection.
End voice with `voice.endSession()` when done. The complete external worker
example adds durable command storage, approvals, and restart handling.

## Run the included rehearsal

From a checkout containing this conference kit, with Rust 1.91, Node 22+, a C/C++
toolchain, and CMake installed:

```sh
bash scripts/setup-rvoip.sh
node scripts/verify-conference-dependencies.mjs
npm ci
npx playwright install chromium
PARLEY_BROWSER_CHANNEL=chromium bash scripts/run-conference-demo.sh rehearsal
```

The Vapi Rust SDK snapshot is included in `vendor/server-sdk-rust`. The setup
script creates the pinned sibling Rvoip checkout and applies the local patch.
The verifier rejects a different baseline, changed patch sources, extra Rvoip
changes, or a changed SDK snapshot. The rehearsal creates fresh temporary state
and uses local provider fixtures with real SIP/browser audio. Screenshots label
that mode. It sends no live calls or texts. Linux may also need Playwright's
system browser dependencies (`npx playwright install --with-deps chromium`).

To run a persistent server, provision participants, use a real AI worker, or
configure live providers, follow the [operator runbook](CONFERENCE_RUNBOOK.md).
The server advertises the baseline, patch fingerprint, experimental profile,
envelope version, and actual control transport separately. The stage displays
these details; a patched release must not be described as stock Rvoip 0.3.12.

## Two contribution paths

**Connect your system.** Add an integration for a service or network you know.
For real-time media, implement the Rvoip `ConnectionAdapter` contract and reuse
core Session, Connection, and bridge primitives. For asynchronous messaging,
implement the host's durable delivery and inbound-event path; the Telnyx outbox
is the current concrete example. A reusable cross-provider messaging connector
trait remains work to contribute, rather than an API already promised here.

**Connect your application.** Use `clients/uctp-js/client.mjs` from Node or a
browser. Negotiate the profile, authenticate with a participant token, read a
Conversation snapshot, and address participant IDs. The external assistant in
`examples/conference-assistant` demonstrates durable commands and reconnect
recovery. Neither a new client nor an agent needs carrier credentials.

Useful next integrations include another SMS provider, a team messaging system,
email, and a different SIP endpoint. These are proposed contributions, not
claims of present support. Start with one operation and a local fixture.

## Connector contribution contract

Describe the supported media, transports, operations, limits, and authentication
requirements. Unsupported actions must produce explicit errors; capability
discovery must not promise effects the connector cannot perform.

Preserve canonical `cid`, `sid`, `connid`, participant ID, request ID, and provider
ID where they apply. A phone number is an endpoint, not a Conversation ID.
Normalize events without erasing their source. Distinguish local acceptance,
provider acceptance, delivery, and an actual human reply.

Authorize endpoint resolution on the server. Attribute inbound activity using
verified provider identity and an unambiguous route. Keep credentials inside
the connector boundary. Check callback signatures and redact credentials from
traces and shared recordings.

Persist effects before submitting them, replay duplicate requests without
resubmission, and represent uncertain outcomes honestly. Test a lost response,
late callback, duplicate callback, cancellation, and restart. Hold ambiguous
inbound messages privately until an authorized routing decision; do not guess
from the most recently active Conversation. Bound live observers and publish a
recovery cursor and lease deadline. For media, prove
bidirectional audio and teardown; a connected signaling state is insufficient.

Keep travel-specific content outside the connector. A connector should work for
another task without knowing about flights, conference organizers, or the demo.

## Share something others can run

Include a short README with the build revision, supported capabilities, a local
fixture, setup commands, and expected events. Include a redacted trace showing a
request, its correlated result, and the eventual external outcome. State which
checks used fixtures and which used real services. Show an unchanged UCTP client
working through the connector when possible.

The complete-scenario rehearsal now saves an inspectable JSON trace alongside
its screenshots in `test-results`. It includes actual journal-linked requests,
correlated replies, and event sequence numbers, with its provider mode labeled.
For a persistent server, use the runbook's **Save the interface evidence** command.
The export retains message and transcript content; review it before sharing.

The public attendee kit is at
[conference.rudeless.ai/kit/](https://conference.rudeless.ai/kit/). It contains a
source snapshot, quickstart, this contract, example code and sharing instructions.
The download is an experimental working-tree snapshot with a file manifest and
checksum; the public GitHub branch may lag it. The QR was generated after checking
the public destination/download and independently decoded with macOS Vision.
Share host/client integration ideas through
[Parley issues](https://github.com/eisenzopf/parley/issues), and media connectors
through [Rvoip](https://github.com/eisenzopf/rvoip). Do not publish participant
tokens, contacts, recordings, database files or the private provisioning bundle.

Suggested closing invitation:

> Bring a system you want to connect, or an agent you want to give a voice.
> Build a connector and share it. Build a UCTP application and show us what it can
> do. Help us improve the protocol through real use. The next connection could
> be yours.
