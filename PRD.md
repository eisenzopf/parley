# PRD — Parley

> **Conference addendum (October 5, 2026):** `CONFERENCE_DEMO_PLAN.md` defines the accepted October 13–15 showcase. It adds an external Vapi assistant worker, explicit multi-person messaging through Telnyx, and UCTP communications control for the worker and browser. These requirements extend the original single-process/two-party v1 scope below. Legacy entry points remain supported. Sandbox travel decisions and live communications are labeled separately.

**Status:** Draft for review
**Date:** 2026-09-15
**Product:** Parley
**Owner:** Rudeless
**Voice plane:** rvoip (`rvoip::app` + Orchestrator + UCTP + `rvoip-vapi`)
**Vocabulary:** voip-3 (`docs/voip-3-conversation-model.md`)
**Client protocol:** UCTP (`docs/CONVERSATION_PROTOCOL.md`)
**Related products:** rvoip (library), Bridgefu (two-leg media gateway), Thelve (workforce / CCaaS OS)

This document is the product contract. Architecture notes, API sketches, and the five-minute demo are here so they can be argued with before code. They are not a license to grow Parley into Thelve.

Companion: [`IMPLEMENTATION_PLAN.md`](IMPLEMENTATION_PLAN.md) — phased, file-level build plan. On conflict, this PRD wins for product; the plan wins for sequencing and file layout.

---

## 0. One sentence

Parley is a UCTP-speaking conversation server: one durable **Conversation** across web chat, PSTN, browser voice, and SMS, with a Vapi AI and humans as peer **Participants**, built as a thin product on rvoip.

If a customer has to repeat themselves because the channel changed, Parley failed. If the public API says `call`, `leg`, `dialog`, or `bot`, Parley failed. If the widget speaks a private JSON protocol instead of UCTP envelopes, Parley failed as a voip-3 showcase.

---

## 1. Summary

Parley is the missing product in the Rudeless stack. Bridgefu proves that rvoip can terminate and bridge **Connections**. Thelve proves that a full business OS can sit above the voice plane. Neither is a small, shippable demonstration of the voip-3 claim: **one Conversation, many Sessions and Messages, humans and AIs as the same kind of Participant, transports as an implementation detail — with those nouns on the wire.**

Parley is that demonstration, and it is also a real product. It is the front door a company publishes — a phone number and a chat widget — that answers as **one Vapi assistant** on voice, chat, and SMS, remembers across channels and time, and can hand the live Session to a human without opening a new ticket, a new call-id, or a new “chat vs voice” record.

Layering, which this PRD treats as load-bearing rather than optional:

```text
Widget / desk / future native app
        │  UCTP envelopes (voip-3 nouns on the wire)
        ▼
Parley  — Conversation matching, pickup, hours, vCon, SMS, Vapi brain
        │  rvoip Orchestrator
        ▼
SIP / WebRTC / Vapi audio WS   ← interop Connections, not UCTP tunnels
```

voip-3 is the vocabulary. **UCTP is how interactive clients speak it.** rvoip is the server that terminates UCTP and gateways SIP, WebRTC, and Vapi into the same Session. Parley is the product-shaped UCTP server: identity-across-time, one human pickup, one Vapi Identity, a widget, a desk, SMS as Messages.

Developers also get an HTTPS JSON control API that **projects the same nouns** for backends (mint tokens, send a Message from a worker, list Conversations). That REST surface is not a substitute for UCTP on the widget or desk.

Parley is **narrow on purpose**. It is not a contact center, not a CRM, not a two-leg recipe catalog, and not a meeting product. The v1 AI runtime is **Vapi only**. The rvoip harness is out of v1.

---

## 2. Product thesis

Voice AI products today treat a phone call as the unit of work and the bot as an audio coupling: RTP in, text out, TTS back, transfer if it fails. Chat products treat a thread as the unit of work and a phone call as a different system. Vapi itself currently splits **voice call**, **Chat API**, and **Twilio SMS** into sessions that expire. Contact centers then glue those systems with tickets. The customer experiences three products. The developer writes gateway code.

voip-3 says the unit of work is a **Conversation**. A voice call is a **Session**. An SMS is a **Message**. Live web chat with presence is a text **Session**. The AI is a **Participant** with `kind: ai` and a **role** that can change. SIP, WebRTC, and QUIC are **Connections** into the same Session.

UCTP (`CONVERSATION_PROTOCOL.md` §1) is explicit: **UCTP is what apps speak**; SIP and WebRTC are what other systems speak; a UCTP server translates at the Session boundary; **UCTP-over-SIP and UCTP-over-WebRTC are not supported** (that would be tunneling). A voip-3 showcase that ships a custom `{type: offer, sdp}` widget and a REST-only chat log has implemented the nouns in a wiki and VoIP 2.0 on the wire.

Parley’s bet:

1. **The Conversation is the product object.** Everything else is how you attach to it.
2. **AI-as-Participant is the differentiator.** v1 that Participant is a Vapi assistant — the same `assistant_id` on voice, chat, and SMS — not three Vapi products glued in the UI.
3. **UCTP is the client protocol.** Widget and desk send `conversation.create`, `session.invite`, `message.send`. REST is for application backends.
4. **The voip-3 runtime is `Orchestrator`, not `RvoipApp`.** See §2.1. Parley uses rvoip-core + UCTP adapters + SIP/WebRTC interop + `rvoip-vapi`. It does not grow a second media stack.
5. **Showcase and utility are the same demo.** If §8 does not work end-to-end, both failed.

The economic argument for a buyer is continuity: one number, one widget, one Vapi assistant, one human overflow, one record. The strategic argument for Rudeless is that rvoip’s Orchestrator and UCTP finally have a face that is not Thelve.

---

## 2.1 Assessment: does rvoip actually implement voip-3?

Short answer: **the spine does. The protocol spec does. The high-level app builder does not. UCTP on the wire is real for Session/Connection/Stream/Message and incomplete for Conversation.**

Read against three documents and the code, not the README slogans:

| Layer | Document / crate | Verdict |
|---|---|---|
| Vocabulary | `docs/voip-3-conversation-model.md` | Source of truth. Six nouns + verbs. AI is a Participant. SIP/WebRTC/QUIC are Connections. |
| Wire | `docs/CONVERSATION_PROTOCOL.md` + `rvoip-uctp` types | Faithful encoding of those nouns. Conversation envelopes exist. Handoff/takeover are not first-class envelope types. |
| Library | `rvoip-core::Orchestrator` | Implements the nouns as Rust types and most lifecycle methods. Missing product verbs: continue/resume, hand off, take over, role change, identity match. |
| High-level builder | `rvoip::app::RvoipApp` | A **customer/employee escalation gateway**, not a voip-3 API. One Conversation minted at process start. Events named `InboundCallAccepted`, `CallEstablished`. `uctp()` fails until wired. |
| Client SDK | `rvoip-client` | Experimental UCTP-over-QUIC: `session.invite` / `session.end`. Not a Conversation client. |

### Vocabulary → wire → code

| voip-3 | UCTP spec | rvoip-core | UCTP coordinator (live dispatch) | `RvoipApp` |
|---|---|---|---|---|
| Conversation open | `conversation.create` / `opened` | `open_conversation` | **Types exist; inbound handler is `_ => Ok(())` (dropped)** | One Conversation at `build()` |
| Conversation continue / resume | Implicit (more envelopes, same `cid`) | Same cid; **no continue/resume API**; no E.164/visitor match | Same | **No.** New PSTN call is not “continue last Tuesday” |
| Conversation close | `conversation.closed` | `close_conversation` + idle closer | Not dispatched | Not the product |
| Session start/end | `session.invite/accept/end/started/ended` | `start_session` / `end_session` | **Yes** | One `Mixed` Session at boot |
| Session upgrade (chat→voice) | `session.update` or new Session | New Session is possible; app layer must choose | `session.update` not in the dispatch match | `EscalationRequested` → bridge to employee (VoIP 2.0) |
| Participant join/leave | `session.participant.joined/left` | `join_session` / `leave_session` (kind+role at join) | Emitted as session events, not conversation-level | Customer vs Employee, not kind/role |
| Participant hand off / take over | **Not in the catalog.** Would be `session.update` or a new type | **No `hand_off` / `take_over` / `set_role`.** Existing participant is not updated on re-join | No | Transfer/escalation to a SIP/WebRTC employee |
| Message | `message.send/delivered/read/history` | `send_message_to_conversation`, `list_messages`, `mark_message_read` | **`message.send` yes** | `on_message` callback; chat is not a text Session |
| Connection | `connection.offer/answer/ready/update/end` | `Connection` + adapters | **Yes** | SIP and WebRTC listeners; UCTP bind rejected |
| Stream | `stream.opened/closed/subscribe` | `MediaStream` | Subscribe/unsubscribe **yes** | Hidden |
| Identity / Device | `auth.*`, step-up | Types exist; `join_session` sets `identity_ref: None` | Auth handshake **yes** | Employee name string |
| AI as Participant | Not a special envelope; AI is a Participant on a Connection | `ParticipantKind::Ai` + `Transport::Vapi` / `InProcessAi` + `attach_ai` (harness) | Vapi is not UCTP; it is an interop adapter | Example 14: Vapi as the “employee” |

### What is actually true in code (2026-09)

**`rvoip-core` is the voip-3 library.** Public types are `Conversation`, `Session`, `Message`, `Participant` (`kind`: human/ai/system/external; `role`: customer/agent/supervisor/observer), `Connection`, `MediaStream`. `Transport` includes `Quic`, `WebTransport`, `WebSocket`, `Sip`, `WebRtc`, `Vapi`, `AmazonConnect`, `InProcessAi`. Events include `ConversationOpened/Closed`, `SessionStarted/Ended`, `ParticipantJoined/Left`, `Message*`, `Connection*`. Bridging, vCon-on-session-end, recording, transcription, conference mixer, and tenant quotas exist.

It does **not** implement continuity (match this CLI to last week’s Conversation), **nor** the voip-3 verbs hand off / take over (role change). `attach_ai` attaches a harness to a **Connection**, which is an audio coupling unless the consumer also `join_session(..., ParticipantKind::Ai, Agent)`.

**`rvoip-vapi` is voice-only and does not create an AI Participant.** `VapiAdapter` is a `ConnectionAdapter` (`Transport::Vapi`, `messaging_enabled: false`). `attach_agent` originates a Vapi Connection that **reuses the caller’s `participant_id`** — so the library currently models “second Connection on the customer,” not `kind: ai`. Tool-call events are parsed; **there is no API to send tool results on the WebSocket.** Transfer/hold/resume/DTMF on the Vapi leg are `NotImplemented`. Chat and SMS are not in this crate.

Parley must therefore: `join_session` a distinct AI Participant itself; treat `attach_agent` as the voice Connection only; run tools over Vapi’s **server-URL HTTP** (Parley responds to `request_human` / `send_message`); use Vapi Chat API for text. Prefer a small upstream patch so originate uses the AI `participant_id` instead of the caller’s.

**UCTP spec implements voip-3 on the wire**, with two honest omissions: continue/resume are implicit, and handoff/takeover are not envelope types. §1 is load-bearing: UCTP is what apps speak; SIP and WebRTC are gatewayed; tunneling is forbidden. §4.3 (WebSocket + co-located WebRTC for media) is the browser path Parley should use.

**UCTP code implements the Session plane, not the Conversation plane.** `UctpCoordinator` dispatches auth, `session.invite/accept/cancel/end`, connection.*, stream subscribe, `message.send`, DTMF, quality, auth refresh, identity step-up. `conversation.create/list/closed` parse as `MessageType` and then fall through to no-op. QUIC / WebTransport / WebSocket adapters exist and are used in e2e tests that call `Orchestrator::open_conversation` **out of band**, then `session.invite`. `rvoip-websocket` is exactly spec §4.3.

**`RvoipApp` is not the high-level voip-3 interface.** It is a packaged two-role gateway: `Role::Customer` / `Role::Employee`, `AssignmentPolicy::fixed`, one Conversation + one Mixed Session for the process lifetime, WebRTC signaling that is **not** UCTP, chat via `on_message`, voice via “escalate to assigned employee.” `RvoipAppBuilder::uctp` returns `AppError::UnsupportedTransport`. Example 12 and 14 are this shape. They prove SIP↔WebRTC↔Vapi bridging. They do not prove voip-3.

### What this means for Parley

1. Build on **`Orchestrator` + adapters**, not on `RvoipApp`’s event vocabulary.
2. Interactive clients speak **UCTP**. Identity match, SMS, Vapi Chat, desk, and widget stay Parley.
3. Parley **v1 is blocked** on the rvoip patches in **§2.2**. Do not paper over them in Parley’s public API.

---

## 2.2 rvoip upstream (Parley-blocking)

Parley will pin an rvoip branch (working name `parley/upstream`) until these land on the train Parley releases against. This section is the contract for that branch. It is voip-3 work in rvoip, not Parley-specific glue. Thelve and example 14 should keep compiling; behavior that was accidentally “AI Connection shares the caller’s Participant” is considered a bug and **may change**.

**Stay in Parley (do not put in rvoip):** E.164 / `visitor_id` matching, hours, voicemail-as-Message, Vapi Chat API, SMS provider, widget/desk, Vapi server-URL tool HTTP, Conversation-level vCon wrap, `RvoipApp` event rename.

**Stay out of this branch:** wiring `RvoipApp::uctp()` (Parley talks to `UctpWsAdapter` directly), Vapi Chat inside `rvoip-vapi`, WebSocket tool-result sender (Parley uses Vapi HTTP server messages), identity-store matching, conference productization.

### UP-1 — Vapi Connection belongs to an AI Participant (Must)

**Bug today:** `VapiAdapter::attach_agent` originates `Transport::Vapi` with the **caller’s** `participant_id`, so the Session has two Connections on one human Participant.

**Target:**

```text
join_session(sid, ai_pid, kind=Ai, role=Agent)   // if not already in Session
originate Vapi Connection with participant_id = ai_pid
bridge(caller_connection, vapi_connection)
```

**API (additive, preferred):**

```rust
impl VapiAdapter {
    pub async fn attach_agent(
        self: &Arc<Self>,
        orchestrator: &Arc<Orchestrator>,
        caller_connection_id: ConnectionId,
        options: VapiCallOptions,
    ) -> RvoipResult<VapiAgentCall>;

    pub async fn attach_agent_for_participant(
        self: &Arc<Self>,
        orchestrator: &Arc<Orchestrator>,
        caller_connection_id: ConnectionId,
        ai_participant_id: ParticipantId,
        options: VapiCallOptions,
    ) -> RvoipResult<VapiAgentCall>;
}
```

- `attach_agent_for_participant` is the Parley path. If `ai_participant_id` is not in the Session, `join_session(..., ParticipantKind::Ai, ParticipantRole::Agent)`. Originate with **that** id. Fail if it is already `kind: human`.
- `attach_agent` (legacy): create a new `ParticipantId`, `join_session` as `Ai`/`Agent`, then the same originate. **Do not** keep the caller-id behavior. Update example 14 and `rvoip-vapi` tests to assert `vapi_connection.participant_id != caller.participant_id` and `kind == Ai`.
- `VapiAgentCall` exposes `ai_participant_id()` as well as the two Connection ids.

**Acceptance:** a unit/mock test where caller Participant is `Human`/`Customer`, Vapi Connection’s `participant_id` is a second id present on the Conversation as `Ai`/`Agent`.

### UP-2 — Participant role verbs (Must)

voip-3: a Participant **hands off** and **takes over**. Today `join_session` sets kind/role only when inserting a **new** Conversation Participant; a second join of the same id does not change role. There is no event for role change.

**API:**

```rust
impl Orchestrator {
    pub async fn set_participant_role(
        &self,
        participant_id: ParticipantId,
        role: ParticipantRole,
    ) -> Result<()>;

    /// Current `agent` in the Session (if any) becomes `observer`.
    /// `to` becomes `agent` (joined as Human/Agent if absent).
    pub async fn take_over(
        &self,
        session_id: SessionId,
        to: ParticipantId,
        to_kind: ParticipantKind,
    ) -> Result<()>;

    /// `from` (must currently be `agent`) becomes `observer`.
    /// `to` becomes `agent`.
    pub async fn hand_off(
        &self,
        session_id: SessionId,
        from: ParticipantId,
        to: ParticipantId,
        to_kind: ParticipantKind,
    ) -> Result<()>;
}
```

**Rules:**

- At most one `role=Agent` per Session after `take_over` / `hand_off` (v1 invariant; extra agents become `Observer`).
- `set_participant_role` is the primitive; the other two are convenience.
- Does **not** move Connections. Does **not** REFER. Does **not** unbridge. Parley mutes Vapi separately.
- `leave_session` is unchanged (presence), not a role change.

**Event** (core + `RvoipCoreCrossCrateEvent`):

```rust
ParticipantRoleChanged {
    conversation_id: ConversationId,
    session_id: Option<SessionId>,
    participant_id: ParticipantId,
    from: ParticipantRole,
    to: ParticipantRole,
    at: DateTime<Utc>,
}
```

**Acceptance:** start Session with AI `Agent`; `take_over(human)`; AI is `Observer`, human is `Agent`; customer Connection id unchanged; event fired twice (or once per changed Participant).

### UP-3 — UCTP Conversation envelopes actually dispatch (Must)

**Bug today:** `MessageType::ConversationCreate|Opened|Closed|List` decode, then coordinator `match` arm `_ => Ok(())`.

Follow the existing adapter pattern (`UctpSessionEvent` → adapter → Orchestrator), including a oneshot reply where the client expects `conversation.opened` (same idea as `BindMediaStreams`).

| Envelope | Direction | Server behavior |
|---|---|---|
| `conversation.create` | C→S | If `cid` is absent/new: `open_conversation`. If `cid` already Open: **continue** (idempotent, no second Conversation). Reply `conversation.opened` with the canonical `cid` and current participants. Honor `policy` / `idle_close_secs` on create only. |
| `conversation.list` | C→S | Query Orchestrator / store for the authenticated identity; reply with the list payload (or a sequence of `conversation.opened` + `ack` with cursor — pick one, document it, match `CONVERSATION_PROTOCOL.md` §7.1 as closely as the current store allows). |
| `conversation.closed` | S→C | Emit when Orchestrator `close_conversation` runs (idle or explicit). Not a client no-op drop. |

Optional if the spec’s “explicit close” has no C→S type yet: add `conversation.close` (C→S) mapped to `close_conversation`, or document that clients send `session.end` until a close type exists. **Recommendation:** add `conversation.close` to the v0 catalog and coordinator; it is the missing verb from voip-3 “Conversation closes.”

Do **not** invent a Parley-only envelope dialect. Same JSON types in `rvoip-uctp` payloads.

`session.invite` with a `cid` of an Open Conversation must attach the Session to **that** Conversation (today tests often `open_conversation` out of band — keep that working, but create-from-invite without a prior create should also open or continue per server policy).

**Acceptance:** UCTP WS or QUIC client sends `conversation.create`, receives `conversation.opened`, then `session.invite` with that `cid`; Orchestrator has one Conversation, one Session. Second `conversation.create` with the same `cid` does not allocate a new id.

### UP-4 — `session.update` role (Should)

UCTP has `session.update { kind, details }` and no `participant.hand_off` type. Map:

```json
{ "type": "session.update", "sid": "...", "payload": {
    "kind": "role",
    "details": { "participant_id": "part_...", "role": "observer" }
}}
```

to `set_participant_role`. Unknown `kind` remains ignored (forward-compat). Parley v1 may call Orchestrator over HTTP/control without this envelope; widget/desk handoff should still be expressible on UCTP.

### UP-5 — `join_session` identity (Should)

`join_session` currently hard-codes `identity_ref: None`. Add `identity_ref: Option<IdentityId>` (new param or overload). Parley can still keep its own identity index; this lets vCons and UCTP `identity_id` line up.

### UP-6 — not in this branch

| Item | Why not |
|---|---|
| `RvoipApp::uctp()` | Placeholder already fails closed. Parley uses `UctpWsAdapter` + Orchestrator. Wiring the app builder is a later rvoip convenience. |
| Vapi Chat/SMS in `rvoip-vapi` | Different Vapi product API; Parley owns text. |
| WS tool-result sender | Parley tools are Vapi **server URL** HTTP. |
| `conversation.continue` / `resume` methods on Orchestrator | Same `cid` is enough; Parley matching is out of rvoip. |
| Identity match by E.164 | Product policy. |
| `attach_ai` harness as Participant | Out of Parley v1. |

### Branch, pin, tests

- **rvoip branch:** `parley/upstream` off the train Parley will pin (call out the merge-base SHA in Parley’s `Cargo.toml` when we pin).
- **Crates:** `rvoip-core`, `rvoip-core-traits` / `infra-common` (new event variant), `rvoip-vapi`, `rvoip-uctp`, QUIC/WS adapter event maps, example 14.
- **Tests:** UP-1 mock in `rvoip-vapi`; UP-2 orchestrator unit test; UP-3 coordinator/adapter test that does not pre-call `open_conversation`.
- **Docs:** crate READMEs + a short note in rvoip `GAP_PLAN.md` that Conversation dispatch and AI Participant attach are no longer “consumer-owned.”
- **Compatibility:** additive APIs except `attach_agent` Participant semantics. Changelog must say the Vapi Connection is no longer attributed to the caller.

Parley v1 **Must** in §23.1 may start against this branch. It must not ship a public API that pretends UP-1–UP-3 exist while still on unmodified 0.3.9.

---

## 3. Portfolio placement

```text
        ┌─────────────────────────────────────────────────────────┐
        │  Thelve — workforce, CRM, queues, skills, learning loop │
        │  (orchestration above the voice plane)                  │
        └───────────────────────────▲─────────────────────────────┘
                                    │ consumes Conversations
        ┌───────────────────────────┴─────────────────────────────┐
        │  Parley — Conversation server (this PRD)                │
        │  public nouns: Conversation, Session, Message,          │
        │  Participant, Connection, Stream                        │
        └───────────────────────────▲─────────────────────────────┘
                                    │ Orchestrator + UCTP + Vapi
        ┌───────────────┬───────────┴────────────┬────────────────┐
        │ rvoip library │                        │ Bridgefu       │
        │ voice plane   │                        │ two-leg bridge │
        └───────────────┘                        └────────────────┘
```

| Product | Sells | Unit of work | voip-3 posture |
|---|---|---|---|
| **rvoip** | Library | Connection / Session primitives | Implements the model |
| **Bridgefu** | Protocol interop | Exactly two call legs | Uses Connections; Conversation is not the product |
| **Parley** | Continuity | One Conversation | *Is* the model, as a product |
| **Thelve** | Operating a workforce | Work item / capability | Uses the model inside a much larger OS |

**Do not duplicate Bridgefu.** If the job is “Vapi SIP in, Amazon Connect out, preserve a correlation id,” that is Bridgefu. Parley may *call* Bridgefu later as an egress Connection. It must not grow a recipe catalog or a two-leg engine.

**Do not duplicate Thelve.** If the job is queues, skills, CRM, QA flywheels, or a 390-capability catalog, that is Thelve. A Parley Conversation may later be referenced by Thelve. Parley must remain usable with neither Thelve nor Bridgefu installed.

**Do not duplicate rvoip examples.** Examples 09 (IVR), 11 (harness + vCon), 12 (chat → SIP voice), and 14 (Vapi agent) are the library proofs. Parley is those proofs plus identity-across-time, a public API in voip-3 nouns, a widget, an operator pickup, SMS as Messages, and a closed vCon — as one deployable product.

---

## 4. Target customers

### 4.1 Primary (v1)

**Developers evaluating rvoip** who need a product they can run, click, and copy an API from. This is the showcase audience. If they cannot explain voip-3 after using Parley, the PRD failed.

**Small operators with one number and one overflow human** — a clinic front desk, a trades shop, a fund, a founder who is tired of voicemail. They want:

- The phone answered.
- The website able to talk.
- A text thread that is the same relationship as the call.
- A person they can escalate to without the caller starting over.

They do not want a CCaaS RFP.

### 4.2 Secondary (v1.x)

**Product teams** who would otherwise stitch Twilio Conversations + Twilio Voice + a voice-AI vendor + a helpdesk. Parley is the conversation substrate; their app is the brand.

**Thelve** as a future consumer: Parley Conversations become an ingress shape Thelve can adopt, not a competing desktop.

### 4.3 Good fit

- Inbound-first (people reach you; you do not spray outbound campaigns).
- One AI persona and one human backup are enough.
- Willing to speak voip-3 in their integration (or use the widget and ignore the API).
- Can terminate SIP (carrier, BYO trunk, or a hosted number Parley is pointed at) and serve a small HTTPS widget.

### 4.4 Poor fit

- Need ACD, skills, SLAs, wallboards, WFM, or CRM as the first purchase — use Thelve.
- Need Amazon Connect / Vapi / SIP protocol bridging as the product — use Bridgefu.
- Need HIPAA / FINRA / PCI-DSS commitments in v1.
- Need outbound predictive dialing, SMS campaigns, or call blasting.
- Need N-way meetings, webinars, or MoQ fan-out as the first feature.
- Need a native mobile softphone as v1 (browser pickup is enough).

---

## 5. Business model

v1 is a **product you can self-host** and a **Rudeless-hosted demo**. Monetization must not dictate architecture, but it must be honest.

**v1 (showcase + self-host):**

- Apache-2.0 or MIT (match rvoip; decide before first public commit).
- No per-seat fee. Operator pays their own carrier, SMS, STT/TTS or Vapi, TLS, and storage.
- Rudeless-hosted public demo tenant for the five-minute narrative. Demo data is disposable.

**Later (optional, not v1 scope):**

- Hosted Parley: pay pass-through plus a conversation or number fee.
- Thelve upsell when the operator outgrows one AI + one human.
- Bridgefu attach when they need a specific two-leg egress (Connect, etc.).

Parley must remain valuable if Rudeless never hosts it. A library showcase that only works as Rudeless SaaS is a Thelve shadow, not a showcase.

**What we will not charge for in the model:** seats, “AI messages,” or per-leg minutes on Parley software. Usage costs are providers’. If hosted Parley exists later, price the Conversation or the provisioned identity, not the SIP transaction.

---

## 6. Vocabulary (normative)

Parley uses voip-3 as its user-facing language. Internal SIP/WebRTC terms may appear in logs and adapter code. They must not appear in the public API, widget copy, or operator UI except in an expandable “transport details” disclosure.

Source of truth for definitions: rvoip `docs/voip-3-conversation-model.md`.

### 6.1 Six nouns

| Noun | Meaning in Parley | v1 examples |
|---|---|---|
| **Conversation** | Durable record of a relationship. Survives channel, participant, and time gaps. The object the operator opens. | “Jane from 415-…, since Tuesday” |
| **Session** | Synchronous bounded engagement. Has a start and an end. | Live web chat; the phone call; the browser voice talk |
| **Message** | Asynchronous atomic event. Does not require simultaneous presence. | SMS in or out; after-hours voicemail drop; a file |
| **Participant** | Entity in the Conversation. `kind` ∈ {human, ai, system, external}. `role` ∈ {customer, agent, observer, …} | Caller; the AI receptionist; the human who picked up; the recorder |
| **Connection** | One Participant’s transport binding into one Session | Customer SIP; customer UCTP/WebRTC; human UCTP; Vapi audio WS |
| **Stream** | One media flow on a Connection | Audio in/out; chat data; (v1.x) screenshare |

**Supporting, not primary:** Identity (who a Participant is across Conversations), Device, Tenant. Parley persists Identity at least as “this phone number” and “this widget visitor key.”

### 6.2 Verbs Parley implements in v1

| Object | Verbs |
|---|---|
| Conversation | open, continue, close |
| Session | start, end |
| Message | send, receive |
| Participant | join, leave, hand off, take over |
| Connection | establish, terminate (renegotiate is rvoip’s problem) |
| Stream | open, flow, pause, close (mute / hold / recording pause) |

**Hand off** means: the current `agent` Participant remains in the Session; role becomes `observer` (or they leave if configured); a new Participant joins as `agent`. The customer’s Connection is unchanged.

**Take over** means: the incoming human becomes `agent`; the AI stops speaking on the customer Stream (mute or detach TTS); the AI may remain as `observer`.

These are not SIP `REFER`s in the product language. rvoip may implement a given handoff with a bridge change rather than a SIP transfer. The operator never has to know which.

### 6.3 Kind and role (v1 closed set)

**kind**

- `human` — customer or operator.
- `ai` — the tenant’s Vapi assistant. Still `ai` when the runtime is a Vapi Connection rather than UCTP.
- `system` — recorder, transcriber, vCon finalizer, hours-router. Never speaks as the company unless explicitly configured (e.g. a recorded greeting played by the system Participant).
- `external` — reserved; not used in v1 UI. For a future Bridgefu/Connect far end that is not “our human.”

**role**

- `customer` — the party who reached us.
- `agent` — currently responsible for talking to the customer. At most one `agent` per Session in v1 (the AI or the human, not both speaking).
- `observer` — present, may receive Streams (audio tap, transcript), may whisper to the agent, does not speak to the customer unless barge is enabled (barge is **not** v1).
- `system` roles are expressed as `kind: system` with role `observer` unless they are playing a greeting, in which case they are briefly `agent` for a prompt and then leave or become observer.

v1 does **not** ship `supervisor`, `coach`, or multi-agent collaboration as first-class roles. An observing AI after handoff is `observer`. That is enough to teach the model.

### 6.4 Chat vs SMS (load-bearing)

voip-3 is explicit: live web chat with presence is a **Session** of medium `text`. SMS is a series of **Messages**.

Parley must not flatten these into one “messages table” in the API even if the operator timeline renders both. The timeline is a view. The nouns stay distinct.

| Ingress | Object |
|---|---|
| Embeddable widget, both parties present | `Session` `medium=text` |
| Customer clicks Talk in the widget | `Session` `medium=voice` **in the same Conversation**; text Session may end or idle |
| PSTN / SIP inbound | `Session` `medium=voice` |
| SMS in/out | `Message` `medium=sms` |
| After-hours voicemail | `Message` `medium=audio` (no live Session, or a short Session that only records) |
| Email | **out of v1** |

### 6.5 Forbidden public words

Do not use as resource names or primary UI labels: call, leg, dialog, bot, ticket, room, mix, conference, peer connection, track, channel (in the telephony sense).

Allowed in disclosures: SIP, WebRTC, SRTP, codec names, `Call-ID` as a Connection attribute.

---

## 7. Product shape

Parley is **one process** in v1 (Bridgefu’s lesson: all-in-one is the only topology that has been real). It speaks:

- **UCTP over WebSocket** — widget and operator desk. Envelopes from `CONVERSATION_PROTOCOL.md` §6. Media for Talk/pickup uses the §4.3 hybrid (UCTP signaling + co-located WebRTC PeerConnection). This is UCTP, not a private `{type:offer,sdp}` protocol.
- **HTTPS JSON** — backend control API that **projects the same nouns** (§12). For servers that mint widget tokens, send Messages, list Conversations. Not the customer protocol.
- **SIP** — PSTN/SBC interop Connection via rvoip-sip. Never UCTP-over-SIP.
- **WebRTC** — only as the UCTP §4.3 media substrate or as an interop Connection for a non-UCTP browser. Prefer UCTP signaling.
- **Vapi** — AI Participant: audio via `rvoip-vapi` WebSocket; chat/SMS via Vapi Chat API. Not an interop “employee.”
- **SMS** — Parley-owned adapter (Twilio or Telnyx). Inbound/outbound Messages. Vapi’s hosted inbound SMS is **not** the system of record.

It stores:

- Conversations, Participants, Messages, Session index, Identity keys (phone, widget `visitor_id`), Vapi chat `sessionId` bound to the Conversation.
- Pointers to recordings and vCons in object storage or local disk.
- Tenant config: number, widget key, Vapi assistant id + API key, human pickup target, hours, greeting, retention.

It does not store: CRM records, tickets, queues, skills.

### 7.1 Default packaged experience (“the desk”)

A new tenant is useful after four bindings:

1. **Inbound voice** — a SIP URI or DID that lands on Parley.
2. **Widget** — a snippet that opens a UCTP WebSocket.
3. **AI Participant** — one Vapi `assistant_id` used for voice, chat, and SMS.
4. **Human pickup** — exactly one target: a browser operator login (UCTP) **or** a SIP URI.

Hours, greeting, recording consent string, and after-hours behavior have defaults. That is the whole v1 setup.

### 7.2 Two consumption modes

| Mode | Who | Protocol |
|---|---|---|
| **Desk / widget** | Operator and customer | UCTP |
| **API** | Developer backend | HTTPS `/v1` nouns + webhooks |

Both modes are the same server. Pickup in the desk is `session.participant` join / take over, also available as `POST /sessions/{sid}/participants`.

---

## 8. The five-minute demo (v1 acceptance narrative)

This section is not marketing. It is the **release gate**. A build that cannot perform this narrative on a laptop plus one DID (or a SIP loopback) is not Parley v1.

### 8.1 Narrative

1. Reviewer opens the widget on a demo site. Widget creates (or continues) a **Conversation**. An **AI Participant** joins a **text Session** as `agent`. Reviewer types “I need to change Friday’s appointment.” AI replies in-thread. Each turn is visible as Session events, not as a separate “chat product.”
2. Reviewer clicks **Talk**. A **voice Session** starts in the **same Conversation**. Same AI Identity, now on an audio Connection (WebRTC). The text Session idles or ends. Conversation id does not change. The timeline shows both Sessions.
3. AI cannot complete the change (demo prompt forces escalation). Reviewer (or a second browser as the operator) hits **Pickup**. Human Participant joins the voice Session as `agent`. AI role becomes `observer`. Customer WebRTC Connection is unchanged. Human hears the customer; customer does not redial. AI can still produce observer transcript / whisper to the operator (whisper is operator-only audio or text; not mixed into the customer Stream in v1 if that is the safer default — see §18.3).
4. Session ends. Next morning (or immediately in the demo clock), Parley sends an SMS **Message** on the same Conversation: “You’re confirmed for Friday 3pm.” Inbound SMS reply appends to the same Conversation.
5. Operator clicks **Close**. Parley emits a **vCon** covering the Conversation’s Sessions and Messages. Download works. Parties include customer, AI, human, system recorder.

### 8.2 What the reviewer must be able to say afterward

Unprompted, in their own words, something equivalent to:

- There was one Conversation.
- Chat and the call were Sessions in it; the SMS was a Message.
- The AI and the human were both Participants; the AI did not “transfer to a ticket.”
- SIP vs WebRTC was not the point.

If they instead say “cool IVR” or “neat SIP-WebRTC bridge,” the UI and API leaked the wrong era.

### 8.3 Demo topology

Laptop path (no carrier):

- Widget → Parley WebRTC
- Operator pickup → Parley WebRTC
- SMS simulated via `POST /conversations/{cid}/messages` or a fake adapter
- Optional: `rvoip-sip` loopback as a second customer Connection

Hosted demo path:

- Real DID → Parley SIP
- Real SMS
- Real widget on a static page

Both paths must exercise the same API. The hosted path is the public showcase; the laptop path is CI.

---

## 9. Personas and user stories

### 9.1 Customer (no account)

- As a customer on the website, I can chat without installing anything.
- As a customer in chat, I can start talking without losing the thread.
- As a customer who called the main number, I reach the same company memory as the person who chatted yesterday from that CLI / that widget identity.
- As a customer handed to a human, I do not repeat the reason I called.
- As a customer after hours, I can leave a voicemail or get an SMS, and it is still this Conversation.

### 9.2 Operator (one human)

- As an operator, I see one timeline per Conversation, not a call list plus a chat list plus an SMS inbox.
- As an operator, I pick up a live Session from the browser with one action.
- As an operator, I see who is `agent` vs `observer` right now.
- As an operator, I can send an SMS into an existing Conversation.
- As an operator, I can close a Conversation and export a vCon.

### 9.3 Developer

- As a developer, I create Conversations, send Messages, start Sessions, and join Participants using those words.
- As a developer, I mint a widget token bound to a `visitor_id` so my app’s user continues the same Conversation.
- As a developer, I receive webhooks for `conversation.*`, `session.*`, `participant.*`, `message.*` — not `call.answered`.
- As a developer, I never configure SDP, ICE, or SIP headers to ship the default desk.

### 9.4 Rudeless / rvoip maintainer

- As a maintainer, I can point at Parley and show Orchestrator + UCTP + Vapi used as a product, not only as example 12/14.
- As a maintainer, I can add a transport later (UCTP) without changing Parley’s public nouns.

---

## 10. Capabilities — in scope for v1

### 10.1 Conversation lifecycle

- Open a Conversation explicitly (`POST /conversations`) or implicitly on first ingress (widget, SIP, SMS) according to tenant policy.
- **Match** an ingress to an existing Conversation:
  - SIP/PSTN: E.164 of the remote party (and, if present, a user-to-user / correlation token).
  - SMS: E.164.
  - Widget: `visitor_id` (app-provided) or Parley-issued durable cookie on the widget origin.
- Continue: new Session or Message on an open Conversation.
- Idle policy: `ephemeral` (close after idle) vs `persistent` (explicit close). Default for identified customers: **persistent**. Default for anonymous PSTN with no Messages after a short Session: **ephemeral** so unknown numbers do not live forever.
- Close: operator or API. Closing ends open Sessions cleanly, then finalizes the Conversation vCon.
- List / get / timeline query with cursor pagination.

**Matching is Parley’s job, not rvoip’s.** rvoip will happily open a new Conversation per PSTN call. Parley supplies the Identity key and `continue`.

### 10.2 Sessions

- Start text Session (widget).
- Start voice Session (widget Talk or inbound SIP).
- End Session (either party, operator, API, hours timeout).
- At most **one live voice Session** per Conversation in v1. A second inbound call from the same number while voice is live: busy, or join as… **no.** Return busy / queued-tone / “we’ll SMS you.” Do not build a conference to dodge this.
- A text Session and a voice Session **may** overlap briefly during escalation from chat to talk; v1 should end or idle the text Session when voice is confirmed up.
- Session `medium` is required and immutable.

### 10.3 Messages

- Inbound SMS → `Message` on matched Conversation (open if needed).
- Outbound SMS from API or operator composer, only to an E.164 already on the Conversation (no cold SMS blast).
- After-hours voicemail stored as `Message` `medium=audio` with transcript if ASR is on.
- Widget file upload: **out of v1** except tiny inline text. (Keep v1 text-only for chat body.)
- Delivery state: accepted / sent / failed. Read receipts: optional, off by default.

### 10.4 Participants

- Customer Participant created on first ingress; reused on match.
- AI Participant joins on Conversation open or on first Session, per tenant `ai.join` policy (`on_open` | `on_session` | `on_voice_only`).
- Human Participant joins on pickup.
- System recorder / transcriber join as `kind=system`, `role=observer` when recording or transcription is enabled.
- Role transition: `agent` → `observer` on handoff; at most one `agent` per Session.
- Leave: human hangs up pickup; AI detaches; customer Connection ends → Session end policy.

### 10.5 Connections and Streams (exposed, not designed by the caller)

- List Connections on a Session (transport, state, codecs as attributes).
- Mute / unmute a Participant’s audio Stream (operator mute of customer is **not** default; operator mute of self is).
- Hold/resume the voice Session (customer hears MOH or silence per config).
- DTMF from PSTN captured as Session events (for future IVR; v1 AI may consume DTMF, operator UI shows digits).
- Developers do **not** POST SDP. Widget and operator SDK do WebRTC against Parley; Parley talks to rvoip.

### 10.6 AI Participant (Vapi only in v1)

v1 AI is **one Vapi assistant** (`assistant_id` + API key) appearing as `kind: ai` in every Conversation. The rvoip harness is out of v1.

`rvoip-vapi` **today** is a developer-preview voice Connection that attributes Vapi to the caller. **After §2.2 UP-1** Parley calls `attach_agent_for_participant(ai_pid, …)` so the Vapi Connection belongs to `kind: ai`. Still true after the patch:

- Voice only on this crate (`messaging_enabled` false). Chat/SMS stay Vapi Chat API in Parley.
- `add_message` is in-call context injection, not a text channel.
- No WebSocket tool-result sender. Tools use Vapi **server URL** HTTP into Parley.
- Vapi-leg transfer / hold / resume / DTMF stay unimplemented. Pickup is UP-2 + mute.

| Medium | How the AI is attached | Surface |
|---|---|---|
| Voice Session | Extra Connection on the AI Participant, bridged to the customer | `attach_agent_for_participant` (§2.2 UP-1), `say`, `add_message`, mute, `end`, events |
| Text Session (widget chat) | No audio Connection | Vapi **Chat API** (`POST /chat` + `sessionId` stored on the Conversation) |
| SMS Messages | Same Chat API | Parley SMS adapter; **not** Vapi inbound Twilio SMS |

**Acceptance:** During chat, voice, and SMS, `GET /conversations/{cid}/participants` shows the **same** AI Participant id (`kind: ai`), not the customer’s id. After pickup, that Participant’s role is `observer` (or they have left). The customer Connection id does not change.

v1 AI behavior:

- Tenant config: `assistant_id`, optional transient assistant JSON, **server URL** pointing at Parley for tools.
- Tools: Vapi **HTTP server messages** into Parley (`request_human`, `send_message`, `close_session`). Return results on that HTTP response. Do not wait for an rvoip-vapi tool-result API. Do not use Vapi’s native Send Text tool as the primary path.
- Context: before `attach_agent` and on each Chat API call, inject a capped Conversation timeline. **This is the continuity feature.**
- On human take over: `mute_assistant`; stop Chat replies to the customer. Default `ai.after_handoff = observe`.
- SMS/chat during a live voice Session: `VapiAgentCall::add_message(role=user, trigger_response=true)`.

Vapi’s own SMS product (Twilio 10DLC, 24h session, customer-initiated only, US↔US) is **out of Parley v1 as a backend**. Parley owns SMS; Vapi owns the brain.

No MCP marketplace. No CRM tools. Optional allowlisted `tool.http` only if it does not delay §8.

### 10.7 Human pickup

- One target per tenant in v1: `browser` (operator users) and/or `sip_uri` (phone). If both configured, try browser first for `delay_ms`, then SIP.
- Ring operator UI; on accept, establish WebRTC Connection into the existing voice Session.
- If no human accepts before `pickup_timeout`: AI continues, or voicemail Message, per policy.
- Pickup is **`Orchestrator::take_over`** (UP-2) plus `mute_assistant`. Default (AI stops speaking to customer).
- Consult-then-take-over (human talks to AI first, customer on hold) is **v1.1**.

### 10.8 Widget

- Snippet + public site key.
- Chat composer, message list, Talk button, mute, end.
- Signaling is UCTP over WebSocket. Talk media is the UCTP §4.3 WebRTC hybrid, not example 14’s private offer/answer JSON.
- `visitor_id` from host page optional; if absent, Parley cookie.
- Theming: colors, title, position. No full white-label CMS.

### 10.9 Operator desk

- Inbox of open Conversations (filter: live voice, waiting pickup, unread Message).
- Timeline (Sessions as blocks, Messages as items, Participant join/leave as events).
- Pickup, send SMS, end Session, close Conversation, download vCon.
- Live observer transcript during voice.
- Config: hours, greeting, AI, pickup target, numbers, widget keys, recording, retention.
- Auth: email/password or magic link for v1; SSO later.

### 10.10 Recording, transcription, vCon

- Recording default: **on for voice Sessions**, with a consent prompt (config string played/shown once per Conversation or per Session).
- Pause/resume API for PCI-style mute of recording (implement the control even if we do not claim PCI).
- Transcription: on when an ASR provider is configured; feeds AI, operator, and vCon `analysis[]`.
- **vCon on every Session end** (rvoip already aims here) **and a Conversation-level vCon (or vCon group / wrapping index) on Conversation close** that references Session vCons plus Messages. Exact envelope: follow `rvoip-vcon`. Signing: opt-in with a tenant key; unsigned JSON is the default so v1 is runnable.
- Recorder is a system Participant. Do not record “out of band” without that join event.

### 10.11 Hours and greetings

- Weekly hours + timezone + holiday skip (simple table).
- Open: AI joins.
- Closed: do not start an AI voice pitch. Offer voicemail Message, optional SMS “we’ll get this in the morning,” optional AI **text-only** if the widget is used after hours (config).

### 10.12 Observability

- Structured logs with `tenant_id`, `conversation_id`, `session_id`, `participant_id`, `connection_id`. Never log full message bodies or transcripts at info in production default.
- Metrics: Conversations opened, Sessions by medium, pickup accept time, AI handoff count, SMS send fail, vCon emit fail.
- Health: SIP bound, WebRTC bound, SMS adapter, disk/object store.

---

## 11. Capabilities — out of scope

Owned by **rvoip** (do not reimplement): SIP stack, WebRTC stack, media graph, codecs, SRTP, ICE, harness traits, vCon schema, Orchestrator bridging.

Owned by **Bridgefu** (do not reimplement): two-leg recipe catalog, Amazon Connect screen pop, make-before-break leg replacement, WHIP attachments as the product, named-route call engine.

Owned by **Thelve** (do not reimplement): CRM, cases, leads, knowledge base as a product, queues, skills, SLAs, WFM, QA rubrics, learning flywheel, capability catalog, AAuth workforce, multi-tenant CCaaS desktop.

**Not v1 for Parley itself:**

- N-way conference, supervisor barge, coaching Conversation (§9.7 of voip-3).
- Cross-device customer move (phone → laptop mid-Session).
- Native UCTP client as the widget transport (WebSocket + WebRTC is v1; UCTP is v1.1 if rvoip-client is ready).
- MoQ / broadcast / “stream this Session to 10k.”
- STIR/SHAKEN as a product feature (pass through if rvoip provides it; do not UI it).
- Video and screenshare.
- Outbound campaigns, predictive dial, cold SMS, email.
- Multi-human hunting beyond one target + timeout.
- Multi-AI collaboration (two `agent` AIs).
- App store of tools / MCP.
- White-label mobile apps.
- Kubernetes split (gateway vs worker). One process.
- Customer’s own IdP (OIDC) — v1.x.
- Guaranteed lawful intercept, HIPAA mode, data residency productization.

If a request is “who should take this among many agents,” it is Thelve. If it is “bridge protocol A to protocol B,” it is Bridgefu. If it is “make this RTP safer,” it is rvoip.

---

## 12. Public API

Base path: `/v1`. JSON. Idempotency-Key on creates that can double (Messages, pickups). Auth: tenant API secret (Bearer) for server; short-lived widget tokens; operator session cookies.

Resource names **are** the nouns.

### 12.1 Conversations

```http
POST   /v1/conversations
GET    /v1/conversations
GET    /v1/conversations/{cid}
POST   /v1/conversations/{cid}/close
GET    /v1/conversations/{cid}/timeline
GET    /v1/conversations/{cid}/vcon
```

Create body (illustrative):

```json
{
  "identity": { "e164": "+14155550111", "visitor_id": "usr_123" },
  "policy": "persistent",
  "participants": [
    { "kind": "human", "role": "customer" }
  ]
}
```

Implicit open on ingress is equivalent to this POST with server-filled identity.

### 12.2 Sessions

```http
POST   /v1/conversations/{cid}/sessions
GET    /v1/conversations/{cid}/sessions
GET    /v1/sessions/{sid}
POST   /v1/sessions/{sid}/end
```

```json
{
  "medium": "voice",
  "direction": "inbound"
}
```

Widget Talk and SIP INVITE are server-side session starts. The POST is for API-originated Sessions (e.g. “now call this customer back” — **callback originate is v1.1**; v1 POST is allowed for text Sessions and tests).

### 12.3 Messages

```http
POST   /v1/conversations/{cid}/messages
GET    /v1/conversations/{cid}/messages
```

```json
{
  "medium": "sms",
  "sender_participant_id": "pty_...",
  "body": "You’re confirmed for Friday 3pm."
}
```

### 12.4 Participants

```http
GET    /v1/conversations/{cid}/participants
POST   /v1/conversations/{cid}/participants
POST   /v1/sessions/{sid}/participants
POST   /v1/participants/{pid}/hand_off
POST   /v1/participants/{pid}/take_over
PATCH  /v1/participants/{pid}
POST   /v1/participants/{pid}/leave
```

Pickup is:

```http
POST /v1/sessions/{sid}/participants
{
  "kind": "human",
  "role": "agent",
  "identity": { "operator_id": "op_..." },
  "connection": { "transport": "webrtc" }
}
```

`hand_off` / `take_over` encode the role transition. Do not expose `transfer`.

### 12.5 Connections and Streams

```http
GET    /v1/sessions/{sid}/connections
GET    /v1/connections/{cnid}
POST   /v1/connections/{cnid}/terminate
GET    /v1/connections/{cnid}/streams
POST   /v1/streams/{stid}/mute
POST   /v1/streams/{stid}/unmute
```

No SDP fields. WebRTC signaling is a **separate widget/operator protocol** under `/v1/realtime/...` implemented by rvoip-webrtc, not reinvented as JSON blobs in the conversation API.

### 12.6 Events (webhooks + desk stream)

Event names follow `object.verb`:

- `conversation.opened` `conversation.continued` `conversation.closed`
- `session.started` `session.ended`
- `message.received` `message.sent` `message.failed`
- `participant.joined` `participant.left` `participant.role_changed`
- `connection.established` `connection.terminated`
- `stream.muted` `stream.unmuted`
- `vcon.ready`
- `pickup.requested` `pickup.accepted` `pickup.timeout`

Payload always includes `tenant_id`, `conversation_id`, and ids of the object. Bodies of Messages are included to server webhooks; widget events are scoped to that Conversation.

### 12.7 Config (operator and API)

```http
GET/PUT /v1/tenant
GET/PUT /v1/tenant/ai
GET/PUT /v1/tenant/pickup
GET/PUT /v1/tenant/hours
GET/PUT /v1/tenant/widget
GET/PUT /v1/tenant/numbers
```

Keep this small. A config surface that looks like a PBX is a failed review.

### 12.8 Versioning and errors

- `/v1` frozen for the showcase; additive fields allowed; no silent rename of nouns.
- Errors: `type`, `title`, `detail`, plus `conversation_id` when known. No SIP status codes as the primary error document.

---

## 13. User experience

### 13.1 Widget

Unambiguous customer verbs: **Message**, **Talk**, **End**. Not “Start video,” not “Call us.”

Timeline is the Conversation. When Talk connects, a voice indicator appears on the same thread (“Voice session started”). When a human picks up, the widget may show “You’re talking with Alex” without mentioning SIP.

### 13.2 Operator desk

Primary navigation: **Conversations**. Not Calls, not Tickets.

Each row: customer identity, last activity, live Session medium if any, waiting-for-pickup badge, AI/human agent badge.

Conversation page:

- Header: identity, open/closed, Participants with kind/role chips.
- Timeline.
- Composer (SMS / if text Session live, in-Session text).
- **Pickup** only when a voice Session is live and the operator is not already the agent.
- Side panel: Connection disclosure (transport, codec) behind “Technical details.”

If the desk ships a “Call” tab, that is a bug.

### 13.3 Empty and edge states

- No Conversations: “Publish a number or a widget.”
- After hours inbound voice: recorded Message, timeline item “Voicemail,” no fake live agent.
- Pickup timeout: banner + AI continues or voicemail, per config.
- Unmatched identity: new Conversation, clearly “new.”

---

## 14. Architecture

### 14.1 Process

```text
                    widget / operator desk
                           │ UCTP / WebSocket
                           │ (Talk: UCTP signaling + WebRTC media, spec §4.3)
                           ▼
┌──────────────────────────────────────────────────────────────┐
│ Parley                                                       │
│  UCTP conversation.*  (product: create/continue/close)       │
│  Identity match       hours    pickup / handoff              │
│  HTTPS /v1            widget tokens    Vapi Chat API         │
│                          │                                   │
│                          ▼                                   │
│                   rvoip-core Orchestrator                    │
│          ┌───────────┼────────────┬────────────┬───────────┐ │
│          ▼           ▼            ▼            ▼           ▼ │
│        SIP        UCTP WS      Vapi audio   SMS adapter      │
│      (interop)   (substrate)  (interop)    (Messages)        │
└──────────────────────────────────────────────────────────────┘
```

Do not route the widget through `RvoipApp`’s `AppEvent` vocabulary. SIP/WebRTC **listeners** from `rvoip::app` may be reused; UCTP must be the `rvoip-websocket` adapter (or an upstream wiring of `RvoipApp::uctp`).

Parley is the consumer the rvoip PRD describes as a CPaaS: *customers see my API; I drive rvoip with their commands.* The commands are Orchestrator methods and UCTP envelopes, not `AppEvent::CallEstablished`.

### 14.2 Mapping to rvoip

| Parley | rvoip |
|---|---|
| Process host | `Orchestrator` + registered adapters. Optional SIP/WebRTC bind helpers from `rvoip::app`, **not** `AppEvent` |
| Conversation/Session/Participant/Message live state | `rvoip-core` Orchestrator + stores |
| Identity match, idle Conversations, Vapi chat session id | Parley DB |
| SIP DID in | `rvoip-sip` / `SipAdapter` |
| Widget + desk signaling | `rvoip-uctp` + `rvoip-websocket` (`UctpWsAdapter`) |
| Widget + desk media | UCTP §4.3 co-located WebRTC (websocket crate `media-webrtc`) |
| Voice AI Connection | `rvoip-vapi::VapiAdapter` |
| Chat/SMS AI brain | Vapi Chat API from Parley (not yet in `rvoip-vapi`) |
| Human pickup | `Orchestrator::take_over` / `hand_off` (§2.2 UP-2) + mute Vapi; second Connection into the same Session |
| Recording / vCon | Orchestrator recording + `rvoip-vcon` on Session end; Parley Conversation wrap on close |
| Bridges | `Orchestrator::bridge_connections` |

**Pin policy:** Parley v1 pins rvoip **`parley/upstream`** (path or git) until UP-1–UP-3 merge; then an exact crates.io version. Same discipline as Bridgefu after that. Document the merge-base SHA.

**Maturity honesty:** SIP is beta-qualified. UCTP, WebRTC, Vapi, and `app` are developer preview. Parley v1 is a developer-preview product. README must say this. Conversation dispatch and AI Participant attach are §2.2, not “consumer-owned forever.”

### 14.3 What Parley adds on top of Orchestrator

rvoip does not (Parley does):

- Match “this E.164 is the same customer as last Tuesday.”
- Speak Vapi Chat / SMS.
- Mint widget tokens or serve a desk.
- Own after-hours voicemail as a Message.
- Own “one human pickup target” as a product.
- Serve an operator timeline UI.
- Expose a frozen `/v1` noun API.

rvoip **will** after §2.2: dispatch `conversation.create`, `set_participant_role` / `hand_off` / `take_over`, attribute Vapi to an AI Participant.

Those are Parley. If we find ourselves implementing codecs, we have fallen through the layer.

### 14.4 Data stores (v1)

- **SQLite** (or Postgres if we already have the muscle memory and want multi-writer later — default **SQLite** to copy Bridgefu’s only-proven topology).
- Filesystem or S3-compatible bucket for audio and vCon blobs.
- No Redis required for v1.

Schema is noun-shaped: `conversations`, `participants`, `sessions`, `connections`, `messages`, `identities`, `events`, `vcons`.

### 14.5 SMS adapter

A trait:

- `send(e164, body) -> provider_id`
- inbound webhook → Parley Message

One implementation in v1 (Telnyx preferred given Thelve/Bridgefu familiarity). Fake adapter for the laptop demo and CI.

### 14.6 Multi-tenancy

Tenancy is structural from day one (`tenant_id` on every row and every log). v1 may **run** single-tenant. Adding tenancy later is how these projects die.

---

## 15. Identity, auth, security

### 15.1 Customer identity

v1 Identity keys, in match order:

1. Explicit `visitor_id` from the embedding app (signed token).
2. E.164 (voice CLI / SMS).
3. Widget first-party cookie, scoped to widget origin.

No attempt at probabilistic “same person, new phone” in v1. Operators may **merge** two Conversations via API (`POST /conversations/{cid}/merge`) if we have time; otherwise merge is v1.1 and operators live with duplicates.

### 15.2 Authn

| Client | Auth |
|---|---|
| Server API | Tenant bearer secret, hashed at rest |
| Widget | Short-lived token minted by the embedding backend or by Parley for the snippet key (rate-limited, origin-locked) |
| Operator | Session after password/magic link |
| SIP ingress | As rvoip allows: source ACL, digest, or mTLS. Default for demo: ACL + known trunk |

### 15.3 Authz (v1)

- Tenant isolation is absolute.
- Operator: full desk on their tenant.
- Widget token: only its Conversation.
- API secret: full tenant.
- No RBAC matrix. Second operator user is allowed; they share the tenant. Fine-grained roles are Thelve.

### 15.4 Trust boundaries

- Do not put long-lived SIP credentials in the widget.
- Do not echo API secrets to the desk frontend.
- Media stays on rvoip paths; Parley HTTP does not proxy raw RTP.
- Webhooks: HMAC secret, timeout, retry with backoff, no customer PII in query strings.

### 15.5 Abuse

- Rate limit widget opens and SMS sends per tenant.
- Max concurrent voice Sessions per tenant (config, default small).
- Reject outbound SMS to numbers not already on a Conversation.

---

## 16. AI lifecycle (detail)

State machine per Conversation (simplified):

```text
[open]
   │ AI join policy
   ▼
[ai_agent] ──request_human──► [pickup_pending]
   │                               │ accept
   │                               ▼
   │                          [human_agent]
   │                               │
   └──(no pickup / timeout)────────┘
   │
   ▼
[session_end] → Conversation still open
   │
   ▼
[closed] → vCon
```

Rules:

- Only one `role=agent` in a live voice Session.
- `participant.role_changed` fires on every transition.
- AI observer after handoff **must not** mix TTS onto the customer Stream.
- Whisper v1: operator-only text in the desk (“suggestion”), not injected audio, unless harness whisper-to-employee is already a boring rvoip path. Prefer text; audio whisper is v1.1.
- Injected context at AI join: last N Messages, last Session summaries, customer Identity keys, hours. No hidden prompt that contradicts tenant config.

Evaluation: CI uses a recorded/fake Vapi WebSocket (existing `rvoip-vapi` mock path) plus a fake Chat API. Live `VAPI_API_KEY` is the public demo. SMS may be fake.

---

## 17. Inbound voice and widget Talk (detail)

### 17.1 PSTN / SIP in

1. INVITE arrives. Parley extracts CLI, DID, optional correlation headers.
2. Identity match → continue or open Conversation.
3. Start voice Session; customer Connection = SIP.
4. If hours closed → greeting + voicemail Message → hang up → no AI pitch.
5. If hours open → AI Participant joins as agent; `attach_agent` bridges customer audio to Vapi.
6. Pickup path as §10.7; human WebRTC Connection joins **this** Session.

Early media: allowed if rvoip provides it; not a Parley feature.

### 17.2 Widget Talk

Same as 17.1 from step 2, with customer Connection = WebRTC. If a text Session is live, it idles. Timeline shows Session switch, same `cid`.

### 17.3 Media policy

v1 codecs: whatever rvoip already bridges for SIP↔WebRTC↔Vapi (G.711 ↔ Opus / PCM). Parley does not expose codec pickers.

---

## 18. Handoff and pickup (detail)

### 18.1 Pickup requested

Sources: AI `request_human` tool, operator unsolicited pickup (barge-without-barge: they take over), API `take_over`.

Effects: `pickup.requested`; ring desk; optional ring SIP URI.

### 18.2 Accept

Human Connection established. `take_over`: human `agent`, AI `observer` or `leave`. Customer Connection id **unchanged** in the API. This is the load-bearing voip-3 demo.

Implementation inside rvoip may be a bridge graph change (customer–AI → customer–human, AI tap remains). Parley must not implement this as “new call, conference both.” Bridgefu’s “exactly two legs” constraint must **not** be copied; after pickup the Session has **two or three Connections** (customer, human, optional AI observer). That is allowed and required. It is still not an N-way product: there is one customer and one human agent.

### 18.3 Observer AI

Default: transcript + desk suggestions. Customer must not hear the AI after take over.

### 18.4 Human hangs up first

Policy: `return_to_ai` (default) or `end_session`. `return_to_ai` is the consult-inverse and is **in v1** because the demo is embarrassing if the call dies when the operator glitches. AI becomes `agent` again; `role_changed` fires.

---

## 19. Continuity rules

These rules are the product.

1. **Same E.164, open Conversation** → inbound voice or SMS continues it.
2. **Same visitor_id** → widget continues it.
3. **Closed Conversation, same identity, within `reopen_window`** (default 7 days) → **continue** (reopen) rather than mint a new cid, unless the operator marked `do_not_reopen`.
4. **Different identity** → different Conversation. No silent merge.
5. **Context passed to AI** is Conversation-scoped, not Session-scoped.
6. **vCon close** is Conversation-scoped for the operator export; Session vCons still emit for rvoip honesty.

Wrong: each call is a new Conversation with a “previous call id” note. That is a helpdesk.

---

## 20. Non-functional requirements

| Concern | v1 bar |
|---|---|
| Scale | Tens of concurrent voice Sessions per process. Not 10k. |
| Latency | Pickup click to human audio < 2s on a LAN demo; PSTN constrained by carrier. |
| Reliability | Crash recovery: in-flight Sessions may drop; Conversations and Messages must not. |
| Disk | SQLite + local blobs acceptable; document backup. |
| Platforms | Linux + macOS for demo; Linux for hosted. |
| Browser | Last two Chrome/Firefox/Safari for widget + desk. |
| License | Match rvoip; no surprise copyleft in the default binary. |
| Docs | README + this PRD + a “voip-3 in Parley” page that restates §6 with screenshots once UI exists. |
| Tests | Laptop §8 path in CI with fakes; SIP loopback if affordable. |

---

## 21. Competitive set (how we are not them)

| Thing | Why they are not Parley |
|---|---|
| Twilio Voice + Conversations | Two products, call-centric REST, bot is a `<Say>` loop or a separate vendor |
| Vapi / Retell / Bland | Voice Session as the universe; chat/SMS continuity is glue; AI is the product, not a Participant among others |
| OpenPhone / Smith.ai | Useful desks; not a voip-3 API; not an rvoip showcase |
| LiveKit Agents / Pipecat | Realtime media + agents; Conversation-across-SMS-and-PSTN-over-time is not the object |
| Bridgefu | Two legs, protocol bridge |
| Thelve | Workforce OS |
| Example 12/14 | Correct library demos, not a product |

Parley wins the review if and only if **Conversation + Participant role change** is obvious. Feature checklists will lose to Vapi and OpenPhone.

---

## 22. Success metrics

### 22.1 Showcase (primary)

- A new engineer can run the laptop demo in < 30 minutes from README.
- After §8, they can sketch the six nouns without the PRD open.
- rvoip README can link Parley as the high-level product example (Orchestrator + UCTP in the wild).
- Zero public endpoints named `calls` or `legs`.

### 22.2 Utility (secondary, still real)

- A friendly tenant can put a DID and a widget in front of a real human overflow and complete a week of inbound without a side helpdesk for that number.
- Pickup actually happens in production at least once (not only in CI).
- A Conversation with text Session + voice Session + SMS Message exists in real data, not only fixtures.

### 22.3 Non-goals as metrics

- Do not measure “minutes of PSTN” as success.
- Do not measure “number of integrations.”
- Do not treat Thelve feature requests as Parley backlog without a written exception.

---

## 23. Phasing

### 23.1 v1 — this PRD

Must:

- rvoip **§2.2 UP-1–UP-3** on branch `parley/upstream` (or equivalent landed crates).
- Noun API §12 (core resources + events).
- Widget chat + Talk.
- Inbound SIP voice.
- Identity match (E.164, visitor_id).
- AI Participant (Vapi) with API-visible kind/role on voice, chat, and SMS.
- Widget/desk speak UCTP over WebSocket.
- Human browser pickup with unchanged customer Connection.
- SMS Messages (real or fake adapter).
- Hours + voicemail Message.
- Session + Conversation vCon export.
- Operator desk timeline.
- Single process, SQLite, tenant_id column.
- §8 demo on laptop.

Should (cut if they threaten Must):

- SIP URI as alternate pickup target.
- AI observer suggestions in the desk.
- Conversation reopen window.
- Recording pause API.
- HMAC webhooks.

Nice:

- Second operator user.
- Theme tokens for widget.
- Merge Conversations.

### 23.2 v1.1

- Consult (human talks to AI, customer on hold) then take over.
- Outbound callback: start voice Session to an existing Conversation’s E.164.
- Audio whisper to operator.
- Postgres.
- OIDC for operators.
- Widget UCTP **QUIC** / `rvoip-client` when that client is honest (WS §4.3 is v1).
- Conversation merge UI.
- `tool.http` allowlisted webhook.

### 23.3 Later (not promised)

- N-way Session (still one Conversation) — only if rvoip conference is a product and Thelve does not already own “rooms.”
- Thelve ingest of Parley Conversations.
- Egress through Bridgefu as an `external` Participant.
- Video.
- Native mobile.
- Hosted billing.

---

## 24. Documentation and naming

- Repo: `parley`
- Binary: `parley`
- Crate: `parley` (Rust server)
- Widget package: `@parley/widget` (if we publish)
- Env prefix: `PARLEY_`
- Demo tenant hostname: `parley.local` / hosted TBD

README must open with the six nouns and the five-minute demo, then install. A README that opens with crate features has failed the showcase.

Legal copy: recording consent, SMS TCPA-ish “only to people who contacted this number,” no HIPAA claim.

---

## 25. Risks

| Risk | Mitigation |
|---|---|
| Becomes mini-Thelve | §11 as a hard filter; pickup is one target |
| Becomes Bridgefu with JSON | No recipe catalog; AI must be a Participant in the API |
| Becomes example 12 with a coat of paint | Identity-across-time, SMS Messages, vCon close, operator desk are not optional |
| rvoip `app` / WebRTC preview quality | Honest maturity label; SIP inbound + fake SMS still prove nouns |
| Harness vs Vapi time sink | Vapi only in v1; harness is later |
| Widget speaks private JSON | Widget/desk must send UCTP envelopes |
| Assuming `RvoipApp` is voip-3 | Build on Orchestrator; see §2.1 |
| `attach_agent` as AI Participant | Parley `join_session`s `kind: ai`; fix Vapi originate `participant_id` upstream |
| Chat modeled as Messages only | §6.4 review gate on the API schema |
| Two-leg instinct at pickup | §18.2: Session may have 3 Connections; still not a conference product |
| Identity merge hell | No probabilistic match in v1 |
| Scope of tools | `request_human`, `send_message`, `close_session` only |

---

## 26. Open questions (for review)

Resolve these before implementation, not during it.

1. **License** — MIT to match rvoip, or something else?
2. **AI path** — **Resolved: Vapi only in v1** (voice via `rvoip-vapi`, chat/SMS via Vapi Chat API). Harness is later.
3. **SMS provider** — Twilio vs Telnyx vs fake-until-hosted? Recommendation: **trait + fake + one real.** Do not use Vapi inbound SMS as the store.
4. **Conversation-level vCon** — wrap Session vCons vs native multi-Session envelope?
5. **UCTP `conversation.*` dispatch** — **Resolved: §2.2 UP-3**, rvoip branch `parley/upstream`. Not a Parley-private dialect.
6. **Default after human hangup** — `return_to_ai` vs `end_session`? Recommendation: **`return_to_ai`.**
7. **Hosted demo** — public DID, or laptop recording is enough?
8. **Operator whisper** — text-only vs audio? Recommendation: **text-only in v1.**
9. **Auth for first operator** — bootstrap token in env vs signup form?
10. **Name collisions** — trademark on “Parley”?
11. **Relationship to Thelve realtime-gateway** — Recommendation: **public rvoip APIs only.**
12. **Inbound busy policy** — Recommendation: **SIP busy + optional SMS Message on the live Conversation.**
13. **Vapi Chat session vs voice call id** — Parley Conversation is source of truth; Vapi sessions are per-medium with injected history.
14. **`attach_agent` participant id** — **Resolved: §2.2 UP-1**. Legacy `attach_agent` also creates an AI Participant; do not keep caller-id attribution.

---

## 27. Glossary

| Term | Meaning |
|---|---|
| **Parley** | This product: a conversation server. |
| **voip-3** | The six-noun model (Conversation, Session, Message, Participant, Connection, Stream). |
| **rvoip** | Rust voice-plane library. Parley is a consumer. |
| **UCTP** | Universal Conversation Transport Protocol: voip-3 nouns on the wire. Widget/desk protocol. |
| **Orchestrator** | `rvoip-core` entry point. The voip-3 library API. |
| **RvoipApp** | Packaged customer/employee gateway in `rvoip::app`. Not Parley’s conversation API. |
| **Vapi adapter** | `rvoip-vapi`. AI as a `Transport::Vapi` Connection for voice. |
| **Desk** | Operator UI. |
| **Widget** | Customer chat + Talk embed. |
| **Pickup** | Human Participant joins a live voice Session as `agent`. |
| **Handoff** | Role transition; customer Connection unchanged. |
| **vCon** | IETF conversation envelope; export artifact. |
| **Identity key** | E.164, visitor_id, or cookie used to continue a Conversation. |
| **Bridgefu** | Sibling two-leg media gateway. Not this. |
| **parley/upstream** | rvoip branch that implements §2.2 (UP-1–UP-3). Parley v1 pins it. |

---

## 28. Decision record (proposed, pending review)

Until the open questions are answered, implementers should treat the following as **proposed defaults**, not lore:

- Parley is a Conversation server on `Orchestrator` + UCTP + Vapi, not a CCaaS and not a B2BUA product.
- Public language is voip-3 only. Interactive clients speak UCTP.
- v1 AI is one Vapi assistant across voice, chat, and SMS.
- v1 = §8 demo, one human target, one process, SQLite.
- Widget chat is a text Session; SMS is Messages.
- Pickup may create a third Connection; it must not create a new Conversation.
- Thelve and Bridgefu are peers, not dependencies, in v1.
- Parley v1 pins rvoip **§2.2** (`parley/upstream`) until UP-1–UP-3 land.
- `RvoipApp` is optional listener glue, never the public model.

When this list conflicts with rvoip’s actual Orchestrator or UCTP behavior, **land §2.2 upstream, then adapt Parley — do not change the public nouns.**
