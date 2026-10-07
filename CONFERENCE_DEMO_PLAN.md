# Conference demo plan: one UCTP interface

Current voice-only rehearsal: [steps and operator setup](docs/CONFERENCE_VOICE_REHEARSAL.md).
SMS is deferred while the campaign is under review; the organizer is called directly.

**Prepared:** October 5, 2026, America/Los_Angeles.  
**Conference:** October 13–15, 2026. Plan against October 13 until the speaking slot is known.  
**Format:** eight-minute live demo, then a 60–90 second interface reveal; exact allotted time remains unconfirmed.  
**Status:** implementation in progress; the source audit below records the starting point. Current verification and remaining gates are in [implementation evidence](docs/CONFERENCE_IMPLEMENTATION_STATUS.md). The [presenter script and stage cues](docs/CONFERENCE_RUNBOOK.md#presenter-script-and-stage-cues) include the opening, exact controls, phone move and closing.  
**Release baseline:** rvoip **0.3.12**, with explicitly versioned patches if required.

## 1. The promise and the release gate

Opening:

> We're going to give David, my AI assistant, one goal—and one communications interface—to coordinate four people across phone calls, browser audio, and text messages. You'll watch him recruit help, bring me into a telephone call from my browser, and send everyone the final arrangements. Every interaction will belong to one Conversation. Then I'll show you the interface he used.

The adoption claim is: **connect existing systems through rvoip; give applications a common UCTP conversation interface.**

### Closing: the connector ecosystem is the reason for UCTP

> What you just saw is one assistant coordinating four people across browser audio, phone calls, and text. The bigger idea is what happens when we make those connections an open-source ecosystem.
>
> Our goal is for the community to build Rvoip connectors for the many ways people already communicate. Agents and human applications connect through UCTP. As the connector ecosystem grows, they can reach more people and services through that same interface, while keeping the Conversation together.
>
> Build a connector, and every compatible UCTP client gains another way to communicate. Build a UCTP client, and you can use the connectors available to it. That's what we're inviting you to help build.

The value compounds on both sides: connector authors integrate a system once against the shared contract; client authors implement that contract once rather than a new provider API for each destination. Telnyx, Vapi, and other service providers can participate behind connectors. The case for UCTP is a shared, independently implementable contract across that ecosystem, beyond the convenience of this particular demo or a single provider's API.

Keep the claim precise: today's demonstrated connectors and transport bindings are the working slice; broad coverage is the open-source direction. “Any transport or medium” is an ambition, not a current compatibility guarantee. UCTP control transport (WebSocket here, optionally QUIC) is distinct from the networks and media reached through connectors (SIP/PSTN, WebRTC audio, SMS). Connectors expose their capabilities and limitations; they do not make an SMS endpoint capable of audio or confer access without authorization.

For the final reveal, keep the real request on screen and illuminate the connector that fulfilled it. Then expand the diagram to show **future community connectors**, explicitly labeled as such. End with two contribution paths: **connect your system to Rvoip** and **connect your agent or human application through UCTP**. The published profile and reference client must be reusable outside the travel scenario, and unsupported capabilities must return explicit errors.

Invite participants to contribute and share: “Bring a system you want to connect, or an agent you want to give a voice. Build a connector and share it. Build a UCTP application and show us what it can do. Help us improve the protocol through real use.” The closing QR destination must offer the demo/quickstart, connector contract and starter example, requested integrations, and a verified place to share contributions. Do not display an invented community URL or a QR code until the real destination is available.

The local contribution kit now starts at [Bring the next connection](docs/CONTRIBUTING_CONNECTORS.md), with a [runbook](docs/CONFERENCE_RUNBOOK.md), experimental profile, reference client, and external assistant example. The stage view includes a “Where this goes next” reveal. Verify the public sharing destination before publishing its QR code.

The conference release must demonstrate all of the following:

1. A separately running assistant receives Jonathan's travel task through the Conversation.
2. It contacts the travel booker and gathers an alternate itinerary. The preferred conference setup is a person answering in Thelve's call-center app, reached through a verified receiving SIP route or its PSTN number. The configured booker's ordinary PSTN phone remains the rehearsal fallback; that person has no native SIP endpoint.
3. It sends an SMS to the organizer, receives a reply, and calls the organizer's real PSTN number.
4. Jonathan joins that existing organizer call from his browser. The organizer's SIP dialog and underlying telephone call remain established during the WebRTC audio handoff. Jonathan then selects **Move to my phone**, answers the callback and presses **1**. His telephone replaces his browser in that same Session while the organizer stays on the original call.
5. Jonathan approves the final sandbox arrangements. The assistant sends individually addressed SMS messages to Jonathan, Alex, the booker, and the organizer.
6. Delivery updates and attributed replies remain attached to the same canonical Conversation ID.
7. The presenter selects an action and reveals its real UCTP request, server execution, and correlated outcome.
8. The assistant controls communications through UCTP. It has no carrier credentials, Parley REST control calls, or in-process access to the Orchestrator.

Travel inventory and booking are clearly labeled sandbox operations. SIP, browser audio, the carrier call, and SMS are live in the live mode. Recorded/replayed material is labeled as such.

An attractive graph with REST or provider-specific tool calls hidden underneath does not pass this gate. Neither does a fabricated `connected` row without verified media.

### Accepted reveal: move Jonathan from browser to telephone

After Jonathan joins Jeff's existing call through Parley WebRTC, he says,
“This Wi-Fi is breaking up. Move this conversation to my phone.” The
**Move to my phone** control requests a callback to Jonathan's provisioned phone.
He answers, hears **“To move your existing conversation to this phone, press
one now”**, and presses **1** before the speaking route changes. The browser
remains the speaking peer while the phone rings, answers and prepares media.
The callback has a 45-second deadline; wrong digits, cancellation or no join
confirmation preserve the browser route. Successful replacement retires browser
media and retains the remote Connection, Conversation, Session and Jonathan's
logical Participant ID. The graph shows the new SIP/PSTN Connection.
The confirmation prompt repeats with negotiated G.711 silence between
utterances. Its outbound sends stop before the speaking bridge commits.

Rvoip-core's existing prepared outbound connection and bridge-peer replacement
APIs implement the media operation. Parley supplies owner authorization,
provisioned destination lookup, durable current-route and callback records,
request replay, explicit join confirmation and cleanup. No new core patch was
needed for the actual local media test. Local tests cover two-way SIP audio,
unchanged remote Connection, wrong digit, cancellation, duplicate request and
control-socket loss during accepted work. Live carrier listening remains a
separate gate. Do not claim gap-free audio before measuring it.

Use the user's two owned phones for the isolated test; do not call Jeff until
he is available. The initial request must reach the server before browser
control connectivity disappears completely. Incoming telephone dial-in with
identity admission remains a separate feature.

## 2. Release baseline: what was actually inspected

The published `rvoip`, `rvoip-core`, `rvoip-uctp`, `rvoip-client`, `rvoip-quic`, `rvoip-websocket`, and `rvoip-sip` 0.3.12 versions were verified through crates.io metadata; those versions are not yanked. Source was inspected at the remote `v0.3.12` tag, whose peeled commit is:

`ca7861af4c9920f6949422ae1f40507bf56ede97`

[Release source](https://github.com/eisenzopf/rvoip/tree/ca7861af4c9920f6949422ae1f40507bf56ede97) · [Published core version](https://crates.io/crates/rvoip-core/0.3.12) · [Published UCTP version](https://crates.io/crates/rvoip-uctp/0.3.12)

Parley currently uses sibling **path dependencies**, not the published release. The sibling checkout inspected during this audit is `af757641d11e9a7b33555c55e3506fe13ebf82ae`, on `release/first-publish-credential-probe`, with workspace version **0.3.11**. Its state must not be described as the 0.3.12 baseline. Existing dependency comments naming `parley/upstream` and `f4532ddf` are stale for that checkout.

Both the Parley working tree and its existing implementation plan already contain substantial uncommitted work. Preserve it. This document adds the conference plan without rewriting that work or switching the sibling checkout.

**First implementation task:** build Parley against a reproducible 0.3.12 dependency set, record failures, and pin every upstream patch to an immutable revision. Record crate release, source revision, envelope version, and negotiated capabilities separately. A patched build must identify itself as “0.3.12 + conference patches,” not stock 0.3.12.

This audit inspected source and existing test coverage. It did not run the release test suite, demonstrate carrier interoperability, or establish that Parley compiles against 0.3.12.

## 3. Bounded conference architecture

```text
External assistant worker             Jonathan's browser
  AI planning + tools                  UCTP control + WebRTC audio
           |                                  |
           +----------- UCTP -----------------+
                           |
                    Parley UCTP host
             membership, durable task/messages,
             endpoint routing, provider outcomes
                           |
                   rvoip-core Orchestrator
                       /               \
                 Telnyx SIP        voice-AI adapter
                    / PSTN        (existing Vapi path)
             booker + organizer

          Parley SMS adapter ----------- Telnyx Messaging
```

### Decisions for the deadline

- **One Conversation, four human Participants, one logical AI Participant.** The same organizer Participant owns their SMS endpoint and telephone endpoint. Endpoint identity, logical participation, and physical connections remain distinct.
- **Separate processes, one communications contract.** Default to a small TypeScript/Node assistant worker using UCTP over secure WebSocket; reuse the existing browser transport code after fixing correlation and event handling. This avoids making native audio client development a dependency of the conference release.
- **Keep the existing Vapi voice integration initially.** The worker owns task state and tool execution; Vapi supplies the voice interaction through the server's existing adapter. Provider tool callbacks terminate at the worker and lead to UCTP requests, rather than directly invoking Parley mutations. Vapi and the worker represent one logical assistant, with explicit connection and authority mappings. Validate this split in the first voice spike; do not assume shared provider memory or transcripts happen automatically.
- **One live voice Session at a time.** Speak to the booker first, end that Session, then call the organizer. Jonathan joins the organizer's Session. Persistent messaging and task observation do not require another voice Session.
- **Booker in Thelve; organizer on an ordinary phone.** Prefer a logged-in booker answering in the call-center app. Qualify Thelve's receiving route and agent availability before selecting it for a rehearsal. A direct SIP route and a PSTN number are distinct ways to reach that app; label the actual path. The booker's configured telephone is the fallback. Jeff remains on PSTN through the Telnyx SIP trunk.
- **Jonathan uses Parley's WebRTC interface.** His browser joins Jeff's existing call after the AI has confirmed the pickup. Thelve's booker may also use its browser softphone, but that is a separate application and media leg; verify its gateway path rather than treating a browser login as a native SIP endpoint.
- **Two active speakers during the browser/PSTN moment.** Preserve the organizer connection; replace the assistant's speaking connection with Jonathan's browser connection. Keep the AI's control connection, but do not require a three-way audio mixer or live AI whisper.
- **Preconfigure the four consenting demo contacts.** No contact discovery, arbitrary outbound destinations, directory product, or group-MMS feature is needed. Final SMS deliveries are individual messages.
- **One server implementation for the conference.** Separate assistant and browser applications may share a JS SDK. Describe them accurately; this does not prove interoperability between independently implemented servers.
- **QUIC is an optional transport milestone for this deadline.** UCTP/WS is a valid protocol demonstration. Add a native reference client on QUIC if the critical gates pass. Label whether QUIC carries control only or control plus media. No QUIC media or migration claims without that actual path and measurements.
- **No routing switch stunt in the main story.** A repeatable post-demo recipe may substitute a local SIP target for the PSTN route with unchanged client operations. This is new-session routing, not seamless carrier replacement mid-call.

The existing PRD's Vapi-only brain and single-process application assumptions need an explicit conference addendum: the worker becomes external, and all demonstrated client communications controls become UCTP operations. Preserve legacy entry points for existing consumers. Record the addendum before implementation; the user's accepted conference scenario is the scope authority for this plan.

## 4. rvoip 0.3.12 audit and upstream work

Paths in this section refer to the pinned 0.3.12 source above. “Present” means source exists, not that the entire conference workflow has been verified.

| ID | Finding in 0.3.12 | Required work / acceptance |
|---|---|---|
| R1 | Core already exposes `open_conversation`, `start_session`, `join_session`, `originate_connection`, `bridge_connections`, `take_over`, and `hand_off`. `crates/foundation/rvoip-core/src/orchestrator.rs`. | Reuse these. Do not rebuild SIP, RTP, WebRTC, or general orchestration in Parley. Role changes alone do not move media. |
| R2 | Core also exposes prepared peer handoff and `replace_bridge_destination`, preserving an ingress Connection and Session. Transport-fenced variants have additional stream requirements. Same source file. | Spike organizer-as-retained-ingress, Vapi-as-old-peer, WebRTC-as-new-peer. Verify concrete adapter support, codec behavior, rollback, and two-way audio. Only add core fixes demonstrated necessary by this test. |
| R3 | `conversation.create/list/close` dispatch is implemented. However, `conversation_ops.rs::fulfill_conversation_create` ignores `_initial_participants`. | Complete authoritative participant admission or a typed admission hook; honor requested membership only after authorization. Ensure a server-side AI identity resolves to `kind: ai`. Do not rely on caller-supplied `from` or `kind` for authority. |
| R4 | `SessionInvite` carries `to`, but stock substrate event translation does not implement a product-level destination directory and dial policy. The WS `InboundInvite` arm discards everything except `cid/sid/from`; the QUIC arm retains routing hints but does not itself provide the proposed participant-to-PSTN service. | Add a substrate-neutral, authenticated application handler for target resolution and invite execution/results. Retain caller envelope ID, recipients, intent, principal, and canonical IDs. Parley resolves participant endpoints; core originates the connection. Duplicate requests must not dial twice. |
| R5 | `message.send` decodes `MessageSend`, converts it to `DataMessage`, requires a bound connection, and emits `DataMessage { connid, message }`. Conversion drops `to` and reply-thread metadata. `state/coordinator.rs` and `payloads/message.rs`. | Preserve message-level recipient/thread context through command dispatch. Support durable Conversation messaging without a live voice connection, plus per-recipient async delivery outcomes. Keep connection data messages compatible; do not overload them invisibly with SMS control. |
| R6 | Core `send_message_to_conversation` fans out to every active Connection. It is not a four-recipient store-and-forward SMS service. | Add or expose targeted message dispatch abstractions only where reusable. Persist offline deliveries and execute Telnyx work in Parley. Never use blanket active-connection broadcast for private recipient-specific updates. |
| R7 | `session.update`, message-history types, and several event types exist in the catalog, but their presence does not mean command execution exists. The coordinator dispatch lacks handlers for the required session-update and message-history workflow. | Specify and implement the minimal join/speaking-peer-change and history/observation profile, including completion/failure events. Unsupported required operations must fail explicitly rather than vanish into the generic no-op arm. |
| R8 | QUIC/WT support `SessionBindingResolver`; the default resolver deliberately namespaces Sessions per physical peer. WS does not expose the same configurable resolver in its inspected config. | Provide equivalent authorized joining across required substrates. Map each authenticated peer's route to one canonical Session only after membership checks. Test two different principals joining legitimately and a third principal being rejected. Never disable ownership protection to make the demo work. |
| R9 | `rvoip-client::call` creates a new Conversation ID per call, rejects URI targets, and its high-level `send_message(cid, body)` is `NotImplemented`. Raw QUIC envelope send/receive and media primitives exist separately. | High-level Conversation handles, explicit existing-cid sessions, messaging, and request correlation are needed for a future native SDK. For the deadline, implement the minimal JS client; do not put the whole Rust SDK rewrite on the critical path. A QUIC reference probe can use lower-level public APIs. |
| R10 | Role changes mutate the Conversation Participant role, and `take_over` promotes to `Agent`; media routing is independent. | Do not turn Jonathan into a contact-center agent merely because he speaks. Use session media contribution/handoff state and preserve business roles. Session-scoped role semantics can be a later protocol/core change if the narrow workflow does not require them. |
| R11 | Authentication, connection ownership, request correlation helpers, media subscription, QUIC/WT/WS adapters, and Vapi's distinct AI Participant helper already exist. | Extend the existing mechanisms. Use a stable principal-to-participant mapping and scoped grants in Parley; do not introduce a second auth or media stack. Test the same common command contract on WS and any shipped QUIC path. |

### Upstream patch grouping

Current filed PRs: [#260 RTCP/SIP](https://github.com/eisenzopf/rvoip/pull/260), [#262 UCTP application profiles](https://github.com/eisenzopf/rvoip/pull/262), and [#263 SIP/Vapi diagnostics](https://github.com/eisenzopf/rvoip/pull/263). They target the next release after review and qualification. The conference keeps its pinned local patches until an upstream release is verified. The organizer's explicit browser invitation action is implemented in Parley and needs no additional Rvoip patch. The original audit and proposed grouping below are historical planning context.

1. **UCTP command context and application handler:** authenticated command hooks, full IDs and recipients, reliable replies, capability gating, target routing seam. Shared implementation in `rvoip-uctp`; thin equivalent wiring in substrate adapters. No Telnyx/Vapi business policy in this crate.
2. **Membership and observation:** initial participant fulfillment, authorized shared-session binding, Conversation snapshot/event subscription, history and cursor recovery.
3. **Session action execution:** explicit browser join and speaking-peer replacement backed by the existing core handoff primitives; report completion only after media readiness.
4. **Client reference and conformance:** reusable JS contract, small independent envelope probe, WS tests; native Rust client improvements and QUIC parity as capacity allows.

Prefer additive capabilities and tests over changing behavior for all existing UCTP clients. Version the profile independently of crate semver. Any experimental names must be marked experimental in documentation and negotiated before use.

### Minimal wire contract to settle on October 6

These are requirements, not assertions that every listed operation already works in 0.3.12:

| Operation | Starting point | Contract to finish |
|---|---|---|
| Create/open Conversation | Existing `conversation.create/opened` | Authorized four-human/one-AI membership; canonical `cid` returned and retained. |
| Observe/rejoin | **Proposed** `conversation.subscribe` profile | Snapshot plus cursor-based authorized events; no REST polling for worker task state. Final type names to be recorded in the upstream spec. |
| Send a message | Existing `message.send` recipient field | Conversation-level send, explicit recipients, optional delivery preference, stable message ID, acceptance vs provider delivery vs human acknowledgment. |
| Read history | Existing history vocabulary | Implement history requests and recipient visibility for an authorized observer. |
| Start remote voice | Existing `session.invite` | Resolve a Participant's endpoint server-side, handle ringing/answered/rejected/busy/timeout/cancel, retain `cid`; distinguish requesting a remote contact from attaching the requesting client's own media. |
| Join existing voice | Existing session and connection negotiation | Admit Jonathan to the authorized canonical Session and negotiate his WebRTC Connection; never create a new organizer call as a shortcut. |
| Yield speaking role | Existing `session.update` as candidate | A capability-negotiated operation that prepares/replaces the peer while retaining organizer ingress; explicit result/error, safe rollback. Exact kind/schema is a spec task, not a magic string in Parley. |
| End voice | Existing `session.end`/connection lifecycle | End the intended Session, retain Conversation and messages, release actual provider/media resources. |
| Share task facts and approval | Typed application messages | Versioned task/fact/approval content inside ordinary messages with visibility and source references. These are application data, not disguised arbitrary RPC commands. |

Do not implement a generic `tool.execute` envelope that merely tunnels private REST calls. The common protocol operations must be usable outside this travel scenario. REST remains suitable for provisioning, administration, and provider webhooks.

Every command carries a request ID; replies use `in_reply_to`. Durable operation records map requests to provider calls/messages. Retrying a timed-out request returns the existing operation status. If a provider submission outcome is unknown, reconcile it before redialing or resending; do not claim exactly-once external effects without provider support.

## 5. Parley changes

### P1. Correct identity, addressing, and durable message routing

Current defects relevant to this scenario:

- `src/runtime.rs::handle_event` assigns incoming data messages to the first open Conversation and deduplicates by body text. It must resolve authenticated Connection → Session → Conversation and deduplicate by stable message/request IDs.
- `src/sms/mod.rs::send` picks the first matching E.164 identity. `outbound(to)` checks membership but does not preserve that chosen recipient through `PostMessage`. This cannot safely deliver four tailored updates.
- Inbound SMS selects the first human Participant. `identities` maps a phone number to one Conversation rather than modeling participation across multiple Conversations.
- `MessageRow`/`messages` have no explicit recipients or per-recipient delivery records. `apply_status` records failures but does not implement the complete success progression used by the proposed UI.
- `create_or_continue` does not fulfill the supplied participant list. New Conversations currently auto-create one customer and one AI.

Add migrations rather than modifying already-applied `001_init.sql`:

- Participant endpoint records: tenant, participant, kind, normalized address, verification/source, reachability configuration.
- Conversation membership/access grants separate from endpoint identity.
- Message recipients and deliveries: recipient, endpoint, provider, provider message ID, attempt, status, timestamps.
- Durable communications operations/outbox: request ID, operation ID, actor, target, result, retry/reconciliation status.
- Inbound provider event IDs for deduplication and a routing association from local/remote number pair to the pending Conversation interaction.
- Task facts/decisions with source message/session/event IDs, visibility, and explicit approval records.
- Ordered event cursor plus `cid/sid/connid`, actor, request/operation IDs, and provider references.

For the conference, prebind each contact's reply route to the one active demo task. For reuse, a person can participate in multiple Conversations: ambiguous replies must be held for disambiguation or use a dedicated routing association/number; never pick the newest or first open Conversation silently.

Keep `conversation_id` as the canonical correlation root. A readable reference such as `BLUE-742` is an alias, not an access token. Keep distinct request, message, delivery, Session, logical Connection, SIP dialog, and transport IDs beneath it. QUIC connection IDs are separate and may change.

### P2. One server-side command execution path

Add `src/uctp_commands.rs` (or a small module directory) implementing the shared upstream handler. Both UCTP and retained REST endpoints call the same application services. The conference assistant and browser use UCTP for demonstrated actions.

Add explicit outbound routing to `src/sip.rs` or `src/routing.rs`: Participant → SIP URI or E.164 trunk route → core originate. Provisioned credentials and caller ID stay server-side. Select the route from configuration; do not hardcode Telnyx call-control JSON into the client.

Replace broad connection-triggered Vapi attachment in `src/vapi_voice.rs` with an explicit session policy. Joining a human must not attach another AI or silently rebridge the organizer. Keep one stable AI identity while provider session IDs change.

Implement handoff as prepare browser media → verify readiness → replace the speaking peer retaining organizer ingress → report success. On failure, preserve/restore the AI path and show the real error. Stop the retired AI audio from reaching the organizer; changing a UI role or muting a label is insufficient.

After the two humans agree, Jonathan approves a visible structured summary. For the deadline this confirmation is the authoritative bridge between spoken agreement and task facts; automatic extraction from a transcript may suggest facts but must not fabricate them.

### P3. External assistant and client library

Proposed new paths:

- `clients/uctp-js/`: transport, typed command helpers, reply correlation, event subscriptions, reconnection/history, capability checks.
- `examples/conference-assistant/`: independently launched Node worker, task state machine, Vapi/model adapter, tool execution through UCTP, provider tool callback endpoint where needed.
- `config/conference.example.toml`: participant aliases, route choices, server URLs; no real contact details or secrets in source.
- `config/conference-scenario.json`: sandbox flight inventory, task rules, budget, expected stages.

Reuse `web/widget/uctp.js` selectively. Its FIFO/per-type response waiters must become request-ID-aware before concurrent SMS results and session events are introduced. Unsolicited events must not consume pending command replies, and unobserved events must not disappear.

Model inference and voice-provider access are allowed backend dependencies of the assistant/runtime. The truthful claim is **one communications control interface**, not that the AI makes no other network requests. The stage map shows the actual Vapi audio WebSocket separately from UCTP control.

A worker network test must demonstrate that no carrier API or Parley REST control access is needed. If the UCTP connection is removed, new communications actions cannot proceed through a hidden fallback path.

### P4. Stage view and interface reveal

Add `web/conference/` with three synchronized views of the same recorded events:

1. **Mission:** constraints, four people, unresolved questions, sourced facts, approval, completion.
2. **Connections:** participant nodes; active Sessions; explicitly labeled signaling and media edges. WebSocket signaling is not WebRTC media. A SIP trunk connects to the PSTN; we do not claim visibility into unobserved carrier internals.
3. **Show UCTP:** intent → actual redacted envelope → dispatched operation → actual outcome. Request IDs and `cid` remain visible. Expand JSON on demand.

Extend `web/widget/trace.js`; do not treat its current small token redactor as a complete projector-safe export. Redact credentials, auth payloads, sensitive headers, ICE secrets, phone numbers except demo-safe labels, and irrelevant private context. Separate a safe stage view from the authenticated developer trace.

Drive edges and status transitions from actual events. Provider acceptance is not delivery; delivery is not human confirmation. A request to connect is not a connected media stream. One Conversation does not make every message visible to every participant.

`src/events.rs` currently broadcasts only tenant, cid, and verb. Extend the persisted journal and UCTP observer projection with ordered, correlated payloads and gap recovery. Inspector backpressure or disconnection must not stop media or silently lose task outcomes. Fix the runtime mirror's exit-on-broadcast-error behavior as part of this work.

The presenter may advance the reveal, reset the sandbox, or select an event. Those controls must not forge provider outcomes or bypass the assistant's communications path.

## 6. Sequencing and calendar

This is an aggressive deadline plan, not a validated effort estimate. It assumes upstream and product work can proceed concurrently by available engineers. The first two days determine feasibility; a single implementer should prioritize the critical gates and omit all stretch work. Calendar targets do not override a failed gate.

| Target (Pacific time) | Deliverable and files | Exit gate |
|---|---|---|
| **Oct 5–6: establish baseline and contract** | Reproducible 0.3.12 dependencies; conference PRD addendum; exact wire profile; upstream R3–R8 design; provider/worker voice spike. Touch `Cargo.toml`, lockfile, CI, upstream spec/handlers. | Existing regression status recorded; two authenticated clients can refer to one canonical Conversation; agreed request/result schemas; confirm the Vapi-worker control/audio split works. |
| **Oct 6–7: first complete UCTP slice** | Implement recipient-preserving command dispatch, membership, operation journal, SMS routing, minimal worker and JS client. | External worker sends one real SMS via UCTP; organizer replies; worker receives the correctly attributed reply in the same cid. No Parley REST control. Duplicate request does not duplicate delivery. Also pass a two-Conversation isolation test. |
| **Oct 7–8: real voice and handoff** | Outbound SIP routing, session event projection, explicit Vapi attachment, UCTP browser join, prepared media replacement. | AI speaks with booker; AI calls organizer; Jonathan joins from browser and both humans hear each other. Organizer dialog/Connection remains stable. Failed browser join leaves organizer reachable through AI. |
| **Oct 8–9: complete task and four-party SMS** | Task facts, approval messages, durable recipient deliveries and callback handling, worker state machine. | All four phones receive the correct individualized update; successful receipts and replies are distinct; late replies still attach after voice ends. No unintended recipient receives another person's private content. |
| **Oct 9–10: stage presentation and reproducible kit** | `web/conference`, correlated inspector, reset/preflight scripts, example client, sandbox fixtures, runbook. | Full scenario and interface reveal work from a clean launch. Every shown communications action links to a real UCTP request and outcome. Repeat without editing code. |
| **Oct 11: feature freeze** | Fix only release-blocking defects; pin source/dependencies/config; record known limits and a labeled backup run. | Three consecutive full live runs pass. Proposed local handoff target: browser media ready to audible two-way bridge within 2 seconds; measure and report actual results. External ringing/SMS delays remain visible and are not assigned guaranteed latency. |
| **Oct 12: rehearsal** | Presenter plus booker/organizer stand-ins; stage audio, network and device setup; backup walkthrough. | Complete within the confirmed slot; opening and reveal are understandable; reset and failure recovery are rehearsed; final build is unchanged unless a blocking defect is found. |
| **Oct 13–15: conference** | Frozen build and working example links. | Run preflight before the speaking slot and use the declared live/fallback mode. |

### Dependencies and responsibility boundaries

`baseline + protocol contract → one UCTP SMS round trip → outbound voice/browser handoff → four-party completion → live reveal → freeze/rehearsal`

- **Upstream owner:** shared UCTP semantics, dispatch, authorization bindings, any demonstrated core/adapter defect, conformance tests.
- **Parley owner:** durable identity/membership, SMS/provider routing, task facts, event journal, worker hosting/configuration, stage UI.
- **Demo operator/presenter:** real endpoint availability, participant rehearsal, audio routing, final slot timing.

These describe engineering responsibilities, not authorization to message people or spawn agents. Do not wait for a full general-purpose SDK or a new published rvoip release if a reviewed immutable upstream patch set is ready.

### Scope cuts if gates slip

Cut first: QUIC migration, WebTransport browser audio, three-way AI audio, a second carrier, dynamic contact discovery, a generalized agent marketplace, federation, automatic itinerary extraction, and elaborate graph animation.

Keep: external UCTP-controlled assistant, one Conversation, actual SIP/PSTN + WebRTC handoff, four individually addressed SMS updates, and a truthful wire reveal.

If the one-SMS UCTP gate has not passed by October 7, reassess the live scope immediately. If the complete core story has not passed by October 10, prepare a shorter live protocol slice plus an explicitly labeled recording of verified portions. Do not silently replace missing UCTP operations with REST while keeping the same stage claim. A fallback is a presentation contingency, not completion of the accepted full-demo gate.

## 7. Test and evidence plan

### Automated contract tests

- Two concurrent Conversations with overlapping contacts, identical message text, and distinct IDs; no cross-routing or body-based deduplication.
- Four explicit recipients; exact per-recipient content, delivery IDs, statuses, and reply attribution; failure of one recipient does not mark the others failed or confirmed.
- Duplicate/delayed/out-of-order provider callbacks; durable outbox restart; unknown provider submission outcome reconciled without blind resending.
- Unauthorized cid/sid/participant references rejected; authorized worker and Jonathan join the same canonical Session across separate credentials.
- A message before any voice Session and after every voice Session ends still works through UCTP.
- Mixed replies and unsolicited events correlate correctly; capability refusal returns a useful error; reconnect recovers journal gaps.
- Outbound invite retry, cancel, busy, no answer, and late answer cannot create orphan calls or duplicate calls.
- Role/speaking-peer change cannot leave stale AI audio audible after the human handoff.
- Repeat the common command contract on WS and any included QUIC substrate; transport differences must not change recipients or semantics.

### Media tests

Use the existing upstream bridge and substrate tests as the base, then add a test for the exact topology:

`Vapi/assistant ↔ organizer SIP` → prepare `Jonathan WebRTC` → replace assistant peer → `Jonathan WebRTC ↔ same organizer SIP`.

Assert actual audio in both directions (distinct identifiable test signals), retained organizer Connection/Session and SIP dialog, browser microphone-denial rollback, no codec/rate mismatch, no duplicate AI attachment, and cleanup when either human ends the interaction. Existing Parley pickup tests checking stored IDs/roles alone are not sufficient evidence.

Relevant upstream suites include `rvoip-uctp/tests/coordinator.rs`, `three_party_media.rs`, `cross_transport_bridge.rs`, `rvoip-quic/tests/multi_session_peer.rs`, and `rvoip-websocket/tests/ws_envelope_sdp_bridge.rs`. Reuse their mechanisms without expanding the demo into a multiparty mixer product.

### Proposed Parley test files

- `tests/uctp_conversation_commands.rs`
- `tests/uctp_sms_recipients.rs`
- `tests/uctp_shared_session.rs`
- `tests/outbound_sip_handoff.rs`
- `tests/conference_scenario.rs`
- `e2e/conference.spec.ts`

These are new work, not existing commands that have already passed. Run the existing `./scripts/ci.sh` regression gate after dependency reconciliation, then the new targeted tests and an opt-in real-provider preflight. Fake mode must never unexpectedly use configured live credentials.

### Claim-to-evidence checklist

| Stage claim | Required evidence |
|---|---|
| One Conversation | Same server canonical cid on sessions, all messages/deliveries, replies, and task decisions. |
| One UCTP communications interface | Worker/browser trace and server journal show each demo command crossing UCTP; no hidden REST control or direct carrier calls by worker. |
| Browser to existing PSTN call | Audio measurement/listening plus unchanged organizer dialog/Connection across handoff. |
| Everyone was updated | Four independent provider delivery records; acknowledgments only where actual replies exist. |
| Existing systems participate | Thelve booker reached through the qualified SIP or PSTN receiving route (or the configured PSTN booker fallback), real PSTN organizer, Jonathan's WebRTC client, and SMS endpoints visible with accurate protocol labels. |
| QUIC used | Actual negotiated QUIC connection and observed payload path; distinguish control-only from media datagrams. |
| Portable interface | Runnable documented client and conformance fixture; if demonstrating route substitution, same client code/new session with only server route configuration changed. No claim of independently proven multi-server interoperability. |

## 8. Deployment, runbook, and handoff to attendees

Use one reachable demo server for Parley/media, with the assistant in a separate process. Existing HTTPS tunnels may serve pages, WebSocket control, and callbacks. They must not be assumed to carry SIP/RTP, WebRTC UDP media, or raw QUIC. Provide and test the required direct media reachability; configure ICE/TURN if the deployed topology needs it. A QUIC listener requires a reachable UDP endpoint and validated certificates.

Before the live rehearsal verify:

- Reachable Thelve booker receiving route and available agent, or the configured PSTN booker fallback; organizer PSTN phone; working outbound SIP trunk and caller ID.
- SMS-capable sender and the four configured recipient devices; successful delivery receipts and reply routing.
- HTTPS microphone permission, headset/room audio, remote audio playback, and echo control.
- Vapi voice/session context, worker tool callbacks, and explicit task facts delivered across the text/voice boundary.
- Reconnect and restart behavior, scenario reset isolation, and a clean end-session path that releases carrier resources.
- Whether any attendee Wi-Fi blocks the chosen path. Use the rehearsed network; do not add network switching during the main demo.

Deliverables:

1. Pinned Parley build and upstream patch list, with version/capability banner.
2. `scripts/run-conference-demo.sh`, `scripts/preflight-conference.sh`, and a scoped reset command.
3. `docs/CONFERENCE_RUNBOOK.md`: stage script, actor lines, timing, device/endpoint checklist, recovery and backup steps.
4. `docs/UCTP_CONFERENCE_PROFILE.md`: supported operations, experimental additions, errors, event examples, limitations, exact tested versions.
5. Runnable assistant client, browser view, and deterministic sandbox mode requiring no paid provider accounts.
6. Opt-in live-provider instructions and an inspectable redacted trace from the released build.

The attendee download opens with the same three actions they saw: **send a message, start a voice interaction, join an existing interaction**. Provisioning details come afterward. Do not lead with the entire rvoip crate graph.

## 9. First implementation work order

1. Reconcile Parley with 0.3.12 and record the baseline regression results.
2. Freeze the small UCTP profile and add the authenticated, recipient-preserving command handler upstream.
3. Correct Parley's identity/message schema and routing; connect Telnyx outcomes back to UCTP.
4. Launch the external worker and prove the one-SMS round trip under a single cid, including a second simultaneous Conversation to catch routing shortcuts.
5. Move directly to the real organizer call and browser handoff spike before investing in the stage UI.

The one-SMS gate is the first proof that the interface is real. The retained-call media handoff is the next proof that the accepted conference story is feasible within the deadline.

## Appendix: source anchors for the audit

- [0.3.12 core operations and handoff](https://github.com/eisenzopf/rvoip/blob/ca7861af4c9920f6949422ae1f40507bf56ede97/crates/foundation/rvoip-core/src/orchestrator.rs)
- [0.3.12 UCTP coordinator dispatch and message handler](https://github.com/eisenzopf/rvoip/blob/ca7861af4c9920f6949422ae1f40507bf56ede97/crates/uctp/rvoip-uctp/src/state/coordinator.rs)
- [0.3.12 message payload conversion](https://github.com/eisenzopf/rvoip/blob/ca7861af4c9920f6949422ae1f40507bf56ede97/crates/uctp/rvoip-uctp/src/payloads/message.rs)
- [0.3.12 Conversation fulfillment](https://github.com/eisenzopf/rvoip/blob/ca7861af4c9920f6949422ae1f40507bf56ede97/crates/uctp/rvoip-uctp/src/conversation_ops.rs)
- [0.3.12 authenticated Session binding](https://github.com/eisenzopf/rvoip/blob/ca7861af4c9920f6949422ae1f40507bf56ede97/crates/uctp/rvoip-uctp/src/state/subscription.rs)
- [0.3.12 WebSocket server translation](https://github.com/eisenzopf/rvoip/blob/ca7861af4c9920f6949422ae1f40507bf56ede97/crates/uctp/rvoip-websocket/src/server.rs)
- [0.3.12 QUIC server translation](https://github.com/eisenzopf/rvoip/blob/ca7861af4c9920f6949422ae1f40507bf56ede97/crates/uctp/rvoip-quic/src/server.rs)
- [0.3.12 high-level client](https://github.com/eisenzopf/rvoip/blob/ca7861af4c9920f6949422ae1f40507bf56ede97/crates/rvoip-client/src/lib.rs)
- [Parley dependency configuration](Cargo.toml), [runtime mirror](src/runtime.rs), [SMS routing](src/sms/mod.rs), [provider statuses](src/sms/telnyx.rs), [schema](migrations/001_init.sql), [desk](web/desk/index.html), [trace viewer](web/widget/trace.js).
