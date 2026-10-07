# Experimental UCTP conversation-control/1

This is a small, reusable Conversation control profile for rvoip 0.3.12 plus the
local conference patch. It is not a claim that stock 0.3.12 implements these
semantics or that an external standards body has adopted them. The shared
upstream `ApplicationHandler` is the extension seam; Parley supplies membership,
durable state, endpoint routing, and provider adapters.

The reference client is `clients/uctp-js/client.mjs` (browser and Node 22+). The
independent consumer is `examples/conference-assistant/worker.mjs`. Neither needs
Telnyx credentials. Vapi inference is a separate dependency of the assistant.

## Transport and negotiation

Each WebSocket text frame is one JSON UCTP v1 envelope. Remote clients use WSS;
cleartext WS is permitted only on loopback by the reference client. The current
patch wires this application profile into the WebSocket adapter. QUIC and
WebTransport parity have not yet been implemented or tested.

1. Send `auth.hello` with bearer in `auth_methods`.
2. Require `auth.challenge.payload.server_capabilities.application_profiles` to
   contain `conversation-control/1` before sending a credential.
3. Send `auth.response` with `method: bearer`, `credential`, and `in_reply_to`
   referencing the challenge. Require a correlated `auth.session` response.
4. Every application request has `payload.profile: conversation-control/1`.

Envelope IDs are command IDs. Replies reference the request through
`in_reply_to`. `cid` identifies the durable Conversation; `sid` identifies a
voice Session, and `connid` identifies a Connection. None is an access token.
Tokens carry one participant subject and the profile scope; all referenced
objects are authorized independently. A participant cannot spoof its sender by
putting a different `from` in the payload.

## Implementation identity

Snapshots and preflight capabilities include an `implementation` object with
`host`, `host_version`, `rvoip_baseline`, `rvoip_revision`,
`rvoip_patch_sha256`, `rvoip_patched`, `profile`, `experimental`,
`envelope_version`, and `control_transport`. The stage banner displays the
baseline plus patch status, profile, envelope version, and WebSocket binding.
The baseline commit and exported patch SHA-256 identify the intended dependency
sources; the local dependency verifier checks them before the scripted gates.
This metadata does not claim QUIC use or attest an arbitrary remote binary.

## Supported operations

| Request | Fields | Result |
| --- | --- | --- |
| `inbox.list` | No `cid`; optional `after` cursor | Administrator only; up to 100 privately held ambiguous SMS entries and a cursor. |
| `inbox.resolve` | No `cid`; `inbox_id`, candidate `conversation_id`, `participant_id`, `verification_note` | Administrator only; persist one attributed inbound message in the selected original candidate and record the routing decision. |
| `conversation.create` | No `cid`; `participants` array | Admin only; `conversation.opened` with authoritative `cid` and participant IDs. |
| `conversation.subscribe` | `cid`, `after` cursor, optional `live` | `conversation.snapshot`: open/closed state, roster, capability flags, up to 500 authorized events, last cursor, and optional live lease metadata. |
| `conversation.preflight` | `cid` | Owner/assistant only; state, active Session count, unsettled SMS count, overlapping open Conversation count, and configuration capabilities. Does not contact providers. |
| `conversation.close` | `cid`, no `sid`/`connid`, `verification_note` | Owner only; `conversation.closed` after all Sessions end and queued/submitting/unknown SMS settle. Retains history and retires inbound reply routing. |
| `conversation.inspect` | `cid`, `request_id` | Owner/assistant only; redacted saved request and correlated response for the stage inspector; secrets in nested fields and SDP authentication/key lines are omitted. |
| `message.send` | `cid`, `msg_id`, explicit participant IDs in `to`, `body`, optional `content_type`, `delivery`, `in_reply_to_msg` | `ack` with accepted message and per-recipient queued delivery IDs. |
| `message.history` | `cid`, optional `after` cursor | Up to 500 authorized messages in insertion order, `cursor`, and `has_more`. The JS `history()` helper drains pages. |
| `session.invite` | `cid`, no `sid`; `medium: voice`, one participant ID in `to`, `purpose` | `ack` with accepted Session and Connection IDs. SIP route is resolved from provisioned membership. |
| `session.end` | `cid`, `sid` | `ack` after core ends the Session. Conversation and messaging remain open. |
| `session.update` | `cid`, `sid`, `kind: join_browser` | Owner only; `connection.offer` with the prepared browser Connection ID, SDP, and explicitly configured client-facing `ice_servers`. Requires an attached assistant in the active Session. |
| `connection.answer` | `cid`, `sid`, `connid`, `substrate_setup: {sdp_type: answer, sdp}` | Apply the owner's browser SDP answer and wait for server-side ICE/DTLS acceptance. The speaking bridge is unchanged until handoff. |
| `session.update` | `cid`, `sid`, `connid`, `kind: handoff_to_browser` | Replace the AI speaking connection with the browser, retaining the remote connection; return the core bridge result. |
| `session.update` | `cid`, `sid`, current browser `connid`, `kind: move_to_phone` | Owner only; prepare a callback to the owner's provisioned SIP route in the same Session. Ack returns the new telephone `connid`; no client-supplied destination is accepted. |
| `session.update` | `cid`, `sid`, callback `connid`, `kind: cancel_phone_move` | Owner only; cancel a pending callback while retaining the browser speaking route. Cannot cancel after media commit. |
| `session.update` | `cid`, interrupted `sid`, `kind: confirm_ended`, `verification_note` | Owner only; records verified remote termination after a server restart and permits another call. |
| `connection.end` | `cid`, `sid`, `connid` | End the owner's browser connection. `session.end` ends the whole voice Session. |

New voice invitations require a running conference SIP adapter. Owner recovery attestation remains available without one. Browser admission
and media handoff are implemented behind `media-webrtc`, which enables the native
Opus backend for transcoding. The explicit Chrome media gate has verified local
Opus-to-G.711 audio in both directions, preservation of the SIP call, microphone
denial and rejection of an unconnected browser without interrupting existing
audio, AI audio retirement, and remote hangup cleanup. This uses a
local Vapi wire fixture and does not prove live Vapi or carrier interoperability.
Ordinary messaging tests use fake SMS. Do not equate voice acceptance or a
signaling-connected state alone with verified bidirectional audio.

The conference application uses an assistant-to-owner `message.send` with
`delivery: chat`, `content_type: application/json` and this body to invite a
browser participant to the existing organizer call:

```json
{"type":"travel.browser_invitation","version":1,"sid":"sess_organizer","retained_connid":"conn_organizer"}
```

This is an application message, not an additional UCTP operation. The external
worker's `request_browser_join` action sends it after the organizer's attributed
confirmation. The stage validates the sender, recipient and original organizer
Connection, then rings and enables **Answer organizer call** only while that
Session remains active. Answering negotiates the existing browser handoff;
the invitation itself does not allocate media or place a telephone call.

The server waits for the SIP stream to become bidirectionally ready before
creating its Vapi call. It accepts the browser's ICE/DTLS connection before
allowing `handoff_to_browser`; a stored offer alone is insufficient. These waits
are bounded. The original SIP/Vapi speaking bridge remains during preparation.

The `phone_handoff` capability requires conference voice and `media-webrtc`.
Phone move is admitted only when the owner's browser is the current speaking
peer. A callback answer is insufficient: the answered connection must receive
DTMF **1**, and its bidirectional audio stream must be ready within 45 seconds.
The conference application plays a repeated confirmation prompt on the pending
callback and sends negotiated G.711 silence between utterances. This is pending
call media; it does not change the Conversation's speaking route. Playback stops
before replacement and on cancellation, deadline or callback termination.
The core bridge replacement retains the remote connection. `phone.speaking`
records `retained_connid`, `retired_connid`, `bridge_id` and `join_confirmed`.
Other journal states include `phone.prepared`, `dialing`, `answered`, `confirmed`,
`committing`, `failed`, `cancelled`, `ended`, `unknown` and `interrupted`.

Replay the exact accepted envelope after an ambiguous request timeout; a new
request cannot bypass an active callback. Accepted work continues after UCTP
socket loss. Reconnect and recover its journal. Unknown effects or restart
interruptions require reconciliation rather than automatic redial. A late
retired-browser event cannot restore its speaking route. Ending either active
telephone peer ends the Session; the Conversation remains open.

`conversation.subscribe` and `conversation.snapshot` are experimental profile
names. `conversation.event` contains a `JournalEvent` in `payload.event`.
Polling snapshots with `after` is supported over UCTP; the worker and stage view
use that path for durable recovery. Optional live observers are limited to 64 per
host and one active subscription per physical peer/Conversation. A second live
request on that pair returns 409; capacity exhaustion returns 429. Snapshot
polling remains available.

A live snapshot returns `subscription.id` and `subscription.expires_at` (RFC3339).
A lease lasts at most 30 seconds and ends no later than token expiry. Events carry
`subscription_id`. Clients must honor the deadline even if they never receive the
best-effort `conversation.subscription_ended` notice. Reconnect or request a new
lease with a fresh envelope ID and the last event cursor actually observed.
Subscription creation replay returns the saved metadata and does not create a
new lease. A slow subscriber waits only in its own bounded task; expiry stops the
task without deleting journal facts. Recover those facts with cursor snapshots.
The reference client exposes lease metadata; its default `snapshot()` polls
without requesting a lease. Cursors are ordered journal sequence values and may skip
numbers because of other Conversations or audience filtering. Drain 500-event
pages before acting on a complete snapshot. Request IDs for reads should be new
each time; replaying an old read returns its cached snapshot.

An example SMS request:

```json
{
  "v": 1,
  "type": "message.send",
  "id": "env_example_1",
  "ts": "2026-10-05T12:00:00Z",
  "cid": "conv_provisioned",
  "payload": {
    "profile": "conversation-control/1",
    "msg_id": "msg_example_1",
    "to": ["part_organizer"],
    "delivery": "sms",
    "content_type": "text/plain",
    "body": "Jonathan's arrival changed. Can we confirm the revised pickup?"
  }
}
```

Each participant can have one provisioned SMS address and one SIP route. A PSTN
destination is a server-provisioned SIP trunk route. The client addresses a
participant, not a phone number, URI, carrier account, or API endpoint.

## Membership, visibility, and outcomes

Owner and delegated assistant can observe the full task. Other participants see
their own messages and addressed deliveries; other participants' endpoint
addresses are omitted. Reply references must name a visible message. SMS sends
and voice invitations require owner or assistant authority. Membership checks
also apply to cached replies and reconnects.

SMS acceptance, carrier acceptance (`sent`), `delivered`, `failed`, `unknown`,
and a human's actual reply are different facts. A fake provider only produces
`sent`. Individual SMS fanout is not group MMS. Final states do not regress on
late duplicate callbacks. Inbound SMS is attributed using the local/remote
number pair from existing outbound deliveries. More than one matching open
Conversation is saved in a private holding inbox and acknowledged to the
provider. Its text is not published to any candidate Conversation. Duplicate
callbacks stay held even if the set of open Conversations later changes.
`inbox.resolve` requires administrator authority, an original candidate, a still
open Conversation with the matching participant endpoint, and a verification
note. Resolution and the resulting message/journal fact commit atomically.
The `message.received` fact includes an explicit `administrator_resolution`
routing annotation. Duplicate callbacks or resolution replays cannot create a
second message. A reused provider message ID with changed held content is
rejected. Unknown number pairs still use the existing nonconference inbound
path.

The client persists mutating envelopes before sending. Repeating the same ID
and content returns the saved result; changing content for that ID is a 409.
Server or provider submission crashes can leave an unknown outcome. The server
does not blindly retry a submission that may already have reached the carrier.
Interrupted submissions are journaled as `unknown` during startup, including
recipient-scoped visibility. This is not an exactly-once guarantee across
independent provider systems.

Startup marks persisted live voice Sessions `interrupted`, preserving their IDs
and recording `session.interrupted`. New invitations remain blocked until the
owner verifies remote termination using `confirm_ended`; its resulting
`session.ended` event has `source: owner_verification` and `verified_by`. The
external worker pauses saved effects and planning while such a Session remains
unresolved. Attestation records human verification; it does not remotely end an
orphaned call. An ordinary `session.end` on that Session returns a conflict.

Errors use correlated `error` envelopes with numeric `code` and `reason`: 400
invalid command; 401 authentication; 403 membership/authority; 409 conflict or
unknown pending result; 429 live observer capacity; 501 unsupported operation; 503 unavailable connector.
Unsupported commands do not silently fall back to REST.

## Task content, separate from the protocol

Travel content is ordinary addressed `message.send` data:

- `content_type: application/json`, body containing `{ "type": "travel.proposal",
  "version": 1, "id": "proposal_…", "summary": "…", "sandbox": true }`.
- Owner approval body: `{ "type": "travel.approval", "version": 1,
  "proposal_id": "proposal_…", "approved": true }`.

The worker checks the authenticated sender's owner role, exact proposal ID,
version, and approval flag. Model output and delivery receipts cannot grant
approval. The four final SMS bodies explicitly identify sandbox arrangements.
No real travel purchase is performed.

## Vapi integration

The worker uses Vapi's non-streaming Chat endpoint with a transient assistant,
no provider communications tools, and validated JSON action proposals. Only its
UCTP client executes communication actions. It supplies attributed Conversation
events on each decision; provider chat memory is not assumed to transfer into
voice calls. Voice calls use the existing rvoip-vapi adapter with an explicit
task purpose, the same logical AI participant, and call-specific transcript
events. `session.speech` reports `{sid, speaker, state, turn, source}` with
`state` equal to `started` or `stopped` and `source` equal to `vapi`. These are
provider observations, not confirmation of task completion. The worker waits
while observed human speech is active and for 1,500 ms after the latest observed
human speech/transcript activity before planning; final transcript fragments do
not necessarily delimit a complete turn.

On organizer calls, the voice assistant also has a zero-argument
`request_browser_join` client tool. It announces the handoff and emits a Vapi
tool event; it cannot choose recipients, phone numbers or Session IDs. The host
binds that event to the existing provider connection and publishes
`session.assistant_actions` (supported actions) and `session.assistant_action`
(`sid`, `participant_id`, `action`, `tool_call_id`, `source: vapi`) into this
experimental application journal. These are intent, not completed handoffs.
The external worker validates the attributed organizer reply, live Session and
finished speech, then sends one `travel.browser_invitation` through its UCTP
client. This transition does not wait for another Vapi Chat planning request.
The owner still answers the browser invitation and explicitly moves to the phone;
the original organizer Connection remains the retained endpoint. Other travel
decisions and the approval proposal continue through the planning interface.

Before submitting effects, the worker validates the whole proposed action batch.
It allows at most three planning attempts with validation feedback for locally
invalid decisions, including unknown participant IDs. No rejected batch is
executed. Accepted and pending batches use durable command replay, not replanning.
Malformed JSON stops the worker. Private state retains rejected decisions for
diagnosis and must not be included in the public contribution kit.

Separate real planning and voice gates and one combined `live-vapi` scenario
have passed. The combined scenario uses synthetic local SIP participants and
fixture SMS; PSTN routing and Telnyx delivery remain separate live gates.

API shape checked against [Vapi non-streaming chat documentation](https://docs.vapi.ai/chat/non-streaming)
on October 5, 2026. Integration tests substitute a local HTTP fixture for Vapi
inference; they exercise the real reference client and local UCTP server.

## Reproduce current gates

```sh
bash scripts/setup-rvoip.sh
npm run test:conference-client
cargo test --no-default-features --features sms-fake,uctp --test uctp_conference
cargo test --no-default-features --features sms-fake,uctp,sip --test uctp_conference
```

Provider credentials, consenting contacts, SIP trunk reachability, and a signed
Telnyx webhook endpoint are required for live gates. See the implementation
status document for what has actually passed; this profile is not a completion
certificate for the conference demo.

## Rehearsal lifecycle

`conversation.preflight` reports `ready_for_new_task` only when the Conversation
is open, has no active Sessions or unsettled SMS submissions, and no other open
conference shares its SMS endpoints. The overlap check is conservative even
before messages have been sent. It returns a count, not other Conversation IDs
or telephone numbers. `sms_configured` means local provider client, verification
key, and sender configuration exist (or fake mode); it does not prove credentials,
callbacks, delivery, or network reachability work.

An owner may close a settled Conversation with a nonempty verification note.
The close is durable, journaled, and replayable. It does not force hangup, cancel
queued delivery, erase history, or resolve an unknown provider outcome. A later
fresh effect receives 409; snapshots/history/inspection remain available, and a
replayed previously accepted request returns its saved outcome without repeating
effects. The external worker exits on a closed snapshot while retaining pending
evidence. A new rehearsal provisions a new Conversation and worker state.

The legacy administrator HTTP close endpoint applies the same settled-state
checks to conference Conversations. Conference voice preparation rechecks open
state transactionally before any outbound activation, including when HTTP close
races an invitation.

Rejected command results are also durable. After resolving a blocker, issue a
new close decision with a new request ID. After a lost response, replay the same
decision instead. Closing retires old routing candidates, but SMS carries no
Conversation ID: a late reply to an earlier rehearsal can still be mistaken for
a new one using the same number pair. Stand-ins must acknowledge the change of
scenario; use distinct sender numbers when overlapping trips are required.
