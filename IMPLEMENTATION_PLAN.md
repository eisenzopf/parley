# Parley — Implementation Plan

**Status:** Draft. Implementable contract for v1.
**Date:** 2026-09-15
**Product contract:** [`PRD.md`](PRD.md) (wins on product conflicts)
**rvoip upstream contract:** PRD §2.2 (UP-1–UP-5)
**voip-3:** rvoip `docs/voip-3-conversation-model.md`
**UCTP:** rvoip `docs/CONVERSATION_PROTOCOL.md`

This plan is ordered so each phase has a gate you can run without the rest of the product. Do not start Parley HTTP/UI until Phase 1 (rvoip `parley/upstream`) is green. Do not invent a private widget signaling protocol. Do not name HTTP resources `calls` or `legs`.

---

## 0. How to use this document

- **Phase N is blocked on Phase N−1’s gate**, except 2a/2b which may proceed in parallel after Phase 1.
- Each phase lists **files to add or touch**, **behavior**, and a **gate** (commands + assertions). If the gate fails, the phase is not done.
- File paths under `parley/` are proposed layout for a new repo at `~/Developer/parley`. File paths under `rvoip/` are the current workspace.
- Defaults from PRD open questions, used here so implementers are not blocked: **MIT license**, **Vapi-only AI**, **SMS trait + fake + one real (Telnyx)**, **`return_to_ai`**, **text-only whisper**, **operator bootstrap token in env**, **public rvoip APIs only**.

---

## 1. Two repositories

```text
Phase 1  ──►  rvoip branch parley/upstream     (library)
Phase 2+ ──►  parley repo, pin that branch     (product)
```

| Repo | Branch | Role |
|---|---|---|
| `~/Developer/rvoip` | `parley/upstream` off the SHA Parley will pin (today: whatever `0.3.9` / `thelve/sip-ingress-guard` train you choose; record it) | UP-1 Vapi Participant, UP-2 role verbs, UP-3 UCTP conversation dispatch; Should: UP-4, UP-5 |
| `~/Developer/parley` | `main` | Conversation server, `/v1`, UCTP host, widget, desk |

Parley `Cargo.toml` pins rvoip with a **git rev or path override** until UP-1–UP-3 merge. Comment the merge-base SHA next to the pin.

Thelve and Bridgefu stay on crates.io until they opt in. Example 14 must keep compiling on `parley/upstream`; its Vapi Connection **must no longer** share the caller’s `participant_id`.

---

## 2. Confirmed v1 shape (do not relitigate in code review)

| Decision | Implementation consequence |
|---|---|
| One process | Single `parley` binary. No gateway/worker split. |
| Orchestrator is the runtime | No `RvoipApp::run()` conversation loop. SIP bind helpers from `rvoip::app` / `rvoip-sip` are OK; `AppEvent` names never appear in Parley logs or HTTP. |
| `RvoipApp::uctp()` stays unwired | Use `rvoip_websocket::UctpWsAdapter` (+ `UctpWsServer` internally). |
| Widget/desk speak UCTP/WS | One JSON envelope per WS text frame (`CONVERSATION_PROTOCOL.md` §4.3). Talk media = co-located WebRTC via `rvoip-websocket` `media-webrtc`. |
| REST is backends only | `/v1` projects the same nouns. Widget does not poll chat over REST. |
| Vapi is the only AI | Voice: `attach_agent_for_participant`. Text/SMS: Vapi Chat API from Parley. Tools: Vapi **server URL** HTTP into Parley. |
| Identity match is Parley | E.164, `visitor_id`, widget cookie. Not rvoip. |
| SQLite + local blobs | `tenant_id` on every row from day one. Single-tenant run is a config of one row, not a schema without tenancy. |
| At most one live voice Session per Conversation | Second INVITE → SIP busy; optional SMS Message on the live Conversation. |
| After pickup: 2–3 Connections | Customer + human + optional muted Vapi. Not Bridgefu two-leg. Not a conference product. |

---

## 3. Target tree (parley)

Create this layout in Phase 2. Do not add extra crates for v1.

```text
parley/
  PRD.md
  IMPLEMENTATION_PLAN.md
  README.md
  LICENSE
  Cargo.toml                 # package parley, bin parley
  Cargo.lock
  rust-toolchain.toml        # match rvoip (document version)
  .gitignore
  config/default.toml
  migrations/001_init.sql
  src/
    main.rs
    lib.rs                   # crate for tests
    config.rs
    error.rs
    ids.rs                   # newtype wrappers only if needed; prefer rvoip ids
    runtime.rs               # Orchestrator + adapters + http + uctp + sip
    auth.rs                  # API bearer, widget tokens, operator sessions
    store/
      mod.rs
      sqlite.rs
      schema.rs
    identity.rs              # match / continue Conversation
    hours.rs
    conversation.rs          # Parley policies on top of Orchestrator
    events.rs                # webhook + desk fan-out (object.verb)
    http/
      mod.rs                 # axum router /v1
      conversations.rs
      sessions.rs
      messages.rs
      participants.rs
      connections.rs
      tenant.rs
      webhooks.rs            # outbound worker
    uctp_host.rs             # BearerValidator + conversation.* product policy
    sip.rs                   # inbound INVITE → identity match → voice Session
    vapi_voice.rs            # attach_agent_for_participant + mute + add_message
    vapi_chat.rs             # POST https://api.vapi.ai/chat
    vapi_tools.rs            # POST /v1/vapi/tools (Vapi server URL)
    sms/
      mod.rs                 # SmsAdapter trait
      fake.rs
      telnyx.rs              # or stub until provider chosen
    pickup.rs
    recording.rs
    vcon_wrap.rs
    observe.rs               # tracing + counters
  web/
    widget/                  # TypeScript, Vite; @parley/widget
    desk/                    # TypeScript, Vite; operator UI
  tests/
    api_conversations.rs
    identity_match.rs
    uctp_create.rs           # needs rvoip UP-3
    demo_script.rs           # PRD §8 laptop path with fakes
  scripts/
    run-laptop-demo.sh
```

**Binary:** `parley` reads `PARLEY_*` env + `config/default.toml`.

**Frontends:** widget and desk are static assets `parley` can serve in demo mode (`http.static_dir`). Production may put them on any HTTPS origin with CORS + widget key origin allowlist.

---

## 4. Phase 1 — rvoip `parley/upstream`

Base: current rvoip checkout. Create branch `parley/upstream`. Do **not** wire `RvoipApp::uctp()`. Do **not** add Vapi Chat to `rvoip-vapi`.

### 4.1 UP-2 first (Orchestrator role verbs)

Role change is needed by UP-1 tests (AI vs customer) and by Parley pickup. Land it before Vapi.

**Touch:**

| File | Change |
|---|---|
| `crates/foundation/rvoip-core/src/events.rs` | Add `Event::ParticipantRoleChanged { conversation_id, session_id: Option<SessionId>, participant_id, from, to, at }`. Debug redaction like other events. Map in `From<Event> for RvoipCoreCrossCrateEvent`. |
| `crates/foundation/infra-common/src/events/cross_crate.rs` | Add matching `RvoipCoreCrossCrateEvent` variant + `event_type()` string `rvoip_core.participant_role_changed`. Follow adjacent `ParticipantJoined` / `ParticipantLeft` exactly (string IDs). |
| `crates/foundation/rvoip-core/src/orchestrator.rs` | Implement `set_participant_role`, `take_over`, `hand_off` as in PRD §2.2 UP-2. Look up Participant on the Conversation (Participant carries `conversation_id`). Reject unknown id. If `from == to`, no-op (no event). `take_over`: every `Agent` in that Session except `to` → `Observer`; `join_session` `to` if missing; then `set_participant_role(to, Agent)`. `hand_off`: require `from` currently `Agent`; then same demote/promote. Do not touch Connections or bridges. |
| `crates/foundation/rvoip-core/src/lib.rs` | Re-export if needed (methods live on Orchestrator). |
| New test module or `crates/foundation/rvoip-core/tests/participant_roles.rs` | Gate below. |

**`set_participant_role` sketch:**

1. Find Conversation containing `participant_id` (index or scan live `conversations`; fail `ParticipantNotFound` if you must add that error variant — otherwise `InvalidState` with a stable string).
2. Read current `role`. Write new `role`. Bump `last_activity_at`.
3. `emit(ParticipantRoleChanged { … session_id: None or the live session that contains this participant })`. Prefer the Session id if the participant is in exactly one Active Session; else `None`.

**Gate:**

```sh
cargo test -p rvoip-core --test participant_roles
```

Assertions:

- `join_session(ai, Ai, Agent)` then `take_over(sid, human, Human)` → AI `Observer`, human `Agent`.
- Customer Connection id (if any) unchanged.
- Two `ParticipantRoleChanged` events (AI and human), or one per changed participant; none with `from == to`.
- `hand_off` from non-agent returns error.
- At most one `Agent` remains in the Session.

### 4.2 UP-1 (Vapi AI Participant)

**Touch:**

| File | Change |
|---|---|
| `crates/extensions/rvoip-vapi/src/agent.rs` | `attach_agent_for_participant`. `attach_agent` creates `ParticipantId::new()`, `join_session(..., Ai, Agent)`, then calls the for-participant path. **Stop copying caller `participant_id`.** `VapiAgentCall::ai_participant_id()`. |
| `crates/extensions/rvoip-vapi/src/lib.rs` | Export new method. |
| `crates/extensions/rvoip-vapi/README.md` | Document the semantic change. |
| `crates/extensions/rvoip-vapi/tests/mock_transport.rs` | Assert Vapi connection participant ≠ caller; Conversation has `kind=Ai`. |
| `examples/14-vapi-agent/src/main.rs` | Still compiles. After attach, log/assert distinct participant ids if the example inspects session state. |
| Changelog / crate docs | Breaking: Vapi Connection is no longer attributed to the caller. |

**`attach_agent_for_participant` steps:**

1. `options.validate()`, `ensure_registered`.
2. Resolve `session_id` from `caller_connection_id` (`session_of`).
3. Load Session; fail if caller connection missing.
4. If Conversation already has `ai_participant_id` with `kind == Human` → `AdmissionRejected` / `InvalidState`.
5. If not in Session: `join_session(session_id, ai_participant_id, Ai, Agent)`.
6. `OriginateRequest::new(session_id, ai_participant_id, "vapi.websocket", Outbound, caps).with_transport(Vapi).with_context(options)`.
7. `bridge_connections(caller, vapi)` as today; supervisor unchanged.

**Gate:**

```sh
cargo test -p rvoip-vapi
# plus existing mock tests; live_smoke remains --ignored
```

### 4.3 UP-3 (UCTP conversation dispatch)

Coordinator today: `conversation.*` types decode, then `_ => Ok(())`. Follow `BindMediaStreams`: event + oneshot so the adapter can call Orchestrator and the coordinator can send `conversation.opened`.

**Touch:**

| File | Change |
|---|---|
| `crates/uctp/rvoip-uctp/src/types.rs` | Add `ConversationClose` if missing; wire `"conversation.close"`. |
| `crates/uctp/rvoip-uctp/src/payloads/conversation.rs` | `ConversationClose { reason_code, reason }` C→S payload if new. |
| `crates/uctp/rvoip-uctp/src/state/events.rs` | `UctpSessionEvent::ConversationCreate { env_id, cid, tenant_id, policy, idle_close_secs, metadata, initial_participants, reply: oneshot::Sender<Result<ConversationOpenedReply, UctpError>> }` and `ConversationList { … }`, `ConversationClose { cid, … }`. Keep payloads small; adapter talks Orchestrator. |
| `crates/uctp/rvoip-uctp/src/state/coordinator.rs` | In the authenticated `match other`: handle `ConversationCreate`, `ConversationList`, `ConversationClose`. Stop dropping them. `require_scope`: treat conversation envelopes as `uctp:session` or add `uctp:conversation` — **do not invent a scope that breaks existing tokens**; default to the same scope as `session.invite` unless auth tests force a new one. |
| `crates/uctp/rvoip-quic/src/server.rs` (and WT/WS equivalents) | On `ConversationCreate`: parse `cid`; if Orchestrator already has Open Conversation with that id, continue; else `open_conversation`; `join` initial participants if provided; send `conversation.opened` via reply. Map `session.invite` `cid` to that Conversation when starting/attaching a Session (if invite currently always creates a new Session under a mystery cid, fix so a known Open `cid` is reused). |
| `crates/uctp/rvoip-websocket/src/server.rs` | Same event map as QUIC. **Required for Parley widget.** |
| New test | `crates/uctp/rvoip-websocket/tests/conversation_create.rs` or uctp crate test: **do not** pre-call `open_conversation`. Client: auth, `conversation.create`, expect `opened`, `session.invite` with that `cid`, Orchestrator shows one Conversation + one Session. Second create with same `cid` is idempotent. |

**`session.invite` + cid:** if `env.cid` is Some and Orchestrator has that Conversation in Open, `start_session(that_cid, medium, …)` rather than a new Conversation. If cid missing, adapter policy: open ephemeral Conversation (document). Parley will always create/continue first.

**`conversation.closed`:** when Orchestrator emits `Event::ConversationClosed`, adapters that own UCTP peers in that Conversation multicast `conversation.closed`. If that fan-out is too large for this branch, **minimum:** respond to `conversation.close` C→S with `conversation.closed` on that peer and call `close_conversation`. Idle-close multicast can follow.

**Gate:**

```sh
cargo test -p rvoip-uctp
cargo test -p rvoip-websocket --test conversation_create
# plus existing uctp e2e that pre-open conversations still pass
```

### 4.4 UP-4 / UP-5 (Should — land if UP-2/UP-3 are still open)

- **UP-4:** coordinator `SessionUpdate` with `kind == "role"` → adapter `set_participant_role`. Unknown kind: ignore (no error).
- **UP-5:** `join_session(..., identity_ref: Option<IdentityId>)`. Update all call sites. Default `None` at call sites you do not understand.

### 4.5 Phase 1 done when

```sh
cd ~/Developer/rvoip
cargo test -p rvoip-core --test participant_roles
cargo test -p rvoip-vapi
cargo test -p rvoip-websocket --test conversation_create
cargo test -p rvoip-sip --offline   # or the repo’s usual sip smoke; must stay green
```

Record `git rev-parse HEAD` for Parley’s pin.

---

## 5. Phase 2 — Parley skeleton + store + `/v1` nouns (no media)

**Depends on:** Phase 1 SHA.

### 5.1 Cargo

`parley/Cargo.toml`:

- edition / rust-version: match rvoip.
- `rvoip` / `rvoip-core` / `rvoip-sip` / `rvoip-uctp` / `rvoip-websocket` (feature `media-webrtc`) / `rvoip-vapi` from git `parley/upstream`.
- `axum`, `tokio`, `serde`, `serde_json`, `sqlx` (sqlite) or `rusqlite` — pick **sqlx + sqlite** unless you want zero async DB; either is fine, one only.
- `tracing`, `uuid`/`ulid`, `chrono`, `argon2` or `blake3` for API secret hashes, `jsonwebtoken` only if you mint widget JWTs (HMAC with tenant secret is enough).
- Features: `default = ["sms-fake"]`, `sms-telnyx` optional.

`rvoip` features: `sip`, `uctp`, `vapi`. Do **not** enable `app` unless you need a SIP bind helper; prefer `rvoip-sip` directly.

License: MIT file at repo root.

### 5.2 Config (`src/config.rs` + `config/default.toml`)

```toml
bind_http = "127.0.0.1:8080"
bind_uctp_ws = "127.0.0.1:7443"
bind_sip = "127.0.0.1:5060"
sqlite_path = "parley.sqlite"
blob_dir = "var/blobs"
tenant_id = "ten_local"
api_secret = "dev-only"          # override with PARLEY_API_SECRET
operator_bootstrap_token = ""    # PARLEY_OPERATOR_BOOTSTRAP
vapi_api_key = ""
vapi_assistant_id = ""
vapi_public_base = "http://127.0.0.1:8080"  # tools server URL advertised to Vapi
sip_advertise = ""
reopen_window_secs = 604800
max_voice_sessions_per_tenant = 16
```

Env prefix `PARLEY_` overrides. Never log secrets.

### 5.3 Schema (`migrations/001_init.sql`)

All tables include `tenant_id TEXT NOT NULL`.

- `tenants` — id, api_secret_hash, created_at, config JSON (ai, pickup, hours, widget, numbers)
- `identities` — (tenant_id, key_type, key_value) UNIQUE → `conversation_id`, `do_not_reopen`, `updated_at`. `key_type` ∈ `e164`, `visitor_id`, `cookie`
- `conversations` — id (rvoip `ConversationId` string), tenant_id, state, policy, opened_at, closed_at, last_activity_at, vapi_chat_session_id, metadata JSON
- `participants` — id, conversation_id, kind, role, identity_ref, display_name, joined_at, left_at
- `sessions` — id, conversation_id, medium, state, started_at, ended_at
- `connections` — id, session_id, participant_id, transport, state
- `messages` — id, conversation_id, from_participant, medium, body, provider_id, state, created_at
- `events` — id, tenant_id, conversation_id, type, payload JSON, created_at (audit/timeline)
- `vcons` — conversation_id or session_id, path, created_at
- `operators` — id, tenant_id, email, password_hash nullable, created_at
- `operator_sessions` — token hash, operator_id, expires_at

SQLite WAL. `sqlx migrate` or run `001_init.sql` on boot if missing.

**Durability rule:** persist Conversation + Message **before** returning HTTP/UCTP ack. Live Session/Connection may be Orchestrator-only until `session.started`, then upsert.

### 5.4 Identity match (`src/identity.rs`)

```rust
fn resolve_ingress(tenant, keys: IngressKeys) -> Match {
  // 1. visitor_id if present
  // 2. e164
  // 3. cookie
}
```

- Hit + conversation Open → `Continue(cid)`
- Hit + Closed + now < closed_at + reopen_window && !do_not_reopen → `Reopen(cid)` (set Open, `conversation.continued`)
- Else `OpenNew`

Never merge two keys onto one Conversation automatically. When opening, write all provided keys to `identities`.

### 5.5 HTTP `/v1` (`src/http/`)

Axum. JSON. `Authorization: Bearer` = tenant API secret **or** operator cookie **or** widget token (scoped).

Implement PRD §12 resources. Forbidden path segments: `calls`, `legs`, `dialogs`, `bots`, `tickets`.

Idempotency: header `Idempotency-Key` on POST messages and pickup; store key → resource id for 24h.

Errors: `{ "type": "...", "title": "...", "detail": "...", "conversation_id": "..." }`.

**Widget tokens:** `POST /v1/widget/tokens` `{ visitor_id?, origin }` with API secret → short-lived JWT/HMAC (≤ 1h) bound to tenant + optional visitor_id + origin. Rate-limit by origin+IP.

**Operator bootstrap:** if no operators, `POST /v1/operators/bootstrap` with `PARLEY_OPERATOR_BOOTSTRAP` creates the first operator.

### 5.6 Conversation service (`src/conversation.rs`)

Wraps Orchestrator:

- `open` → `orchestrator.open_conversation(tenant, Persistent|Ephemeral, metadata)` + insert sqlite + identity keys + join customer Participant.
- `continue` → reuse cid, `last_activity_at`.
- `close` → end Active Sessions, `orchestrator.close_conversation`, wrap vCon (Phase 9 can stub file write).

AI Participant: on open or on first session per `tenant.ai.join` (`on_open` default for widget; `on_session` for PSTN-only). `join_session` not required until a Session exists; still insert sqlite participant `kind=ai` `role=agent` so the API shows them.

### 5.7 Phase 2 gate

```sh
cargo test -p parley --test api_conversations --test identity_match
```

- `POST /v1/conversations` with e164, `GET` same cid.
- Second POST with same e164 continues (same cid).
- `POST /messages`, `GET /messages` — medium `sms` even if fake (no provider send yet).
- Timeline mixes Session placeholders and Messages without flattening schema.
- No route named `call`.

---

## 6. Phase 3 — UCTP host (widget signaling, no Vapi yet)

**Depends on:** Phase 1 UP-3, Phase 2.

### 6.1 Server

`src/uctp_host.rs` + `runtime.rs`:

1. `Orchestrator::new`.
2. `UctpWsAdapter` / config bind `bind_uctp_ws` with TLS in prod (`wss` feature if the crate needs it; demo may be `ws://127.0.0.1` on localhost only).
3. `BearerValidator` implementation: widget JWT and operator session tokens. Subject → Parley `Identity` / `visitor_id` / `operator_id`. Tenant from token.
4. Register adapter on Orchestrator.

**Product policy on `conversation.create`:** after UP-3 adapter calls `open_conversation`, Parley must still **identity-match**. Implement a Parley hook:

- Option A (preferred): UCTP metadata `{ "visitor_id", "cookie" }` on create; `uctp_host` intercepts `UctpSessionEvent::ConversationCreate` **before** or **instead of** default adapter open — call `identity::resolve_ingress`, then `open_conversation` or continue existing cid, reply `opened` with **Parley’s** cid.
- Option B: let adapter open, then Parley merges — **rejected** (two cids).

If the websocket adapter owns create internally after UP-3, add a **`ConversationPolicyHook`** in rvoip only if you cannot intercept events. Prefer intercepting in Parley via a custom event consumer if the adapter emits create before open. If UP-3 adapter always opens a new cid, **extend UP-3** so create may include `cid` for continue, and Parley supplies it after match (client may omit cid; server fills). Document the chosen path in a comment next to `uctp_host.rs`.

**Minimum viable:** widget always hits REST `POST /v1/conversations` with visitor_id first (API secret from site backend, or widget token that allows create), then UCTP `conversation.create` **with that cid**. That unblocks Phase 3 without a hook. **Showcase purity:** UCTP-only create should work by Phase 7. Implement REST-first create in Phase 3; UCTP-first create in Phase 7.

### 6.2 Browser client (`web/widget/src/uctp.ts`)

Implement a tiny client:

- Connect `ws://host/uctp` or the printed UCTP WS URL (query token).
- `auth.hello` → `auth.challenge` → `auth.response` (bearer = widget token).
- Helpers: send envelope `{ v:1, type, id: ulid, ts, cid, sid, connid, payload }`.
- One envelope per WS text frame.

Do **not** send `{type:"offer",sdp}`. Talk comes in Phase 7 with `session.invite` medium voice + `connection.offer` as rvoip-websocket expects (`connection.offer.substrate_setup` / ICE inside UCTP payloads). Copy field names from `rvoip_uctp::payloads` and from `crates/uctp/rvoip-websocket` tests (`ws_bridge_flow.rs`).

### 6.3 Text Session without Vapi

On widget chat: `session.invite` medium `text` (or Parley `start_session(cid, TextChat)` when invite lands). Persist Session. Echo: `message.send` stored; until Vapi Chat (Phase 4), reply with a stub Participant `ai` body `"…"` only in `PARLEY_AI_STUB=1` tests — **do not ship stub replies in default config**.

### 6.4 Phase 3 gate

- Integration test: mint widget token, WS auth, `conversation.create` with cid from REST, `session.invite` text, `message.send`, sqlite has Message + Session `TextChat`.
- Packet/log: WS frames are UCTP `type` strings, not `offer`/`answer`.

---

## 7. Phase 4 — Vapi brain (chat + tools; voice attach)

### 7.1 Chat (`src/vapi_chat.rs`)

- `POST https://api.vapi.ai/chat` with `assistantId`, `input`, `sessionId` if stored.
- Persist `vapi_chat_session_id` on Conversation.
- Map output text → Message from AI Participant (`medium=chat` or body on text Session — **text Session turns are Session events + stored as messages with `origin=ai` for timeline**; sqlite `messages.medium` = `chat` vs `sms`). Do not use `medium=sms` for widget.

Inject context: first chat in a Conversation, prefix input with capped timeline (last N messages, last session summaries). Hard cap tokens (e.g. 8k chars).

Fake: `PARLEY_VAPI_CHAT=fake` returns deterministic strings for CI.

### 7.2 Tools (`src/vapi_tools.rs` + HTTP)

Vapi assistant **server URL** = `{vapi_public_base}/v1/vapi/tools`.

Handle Vapi server messages (tool-calls). Implement:

| Tool | Effect |
|---|---|
| `request_human` | `pickup::request(cid, sid)` → `pickup.requested` |
| `send_message` | only if `to` E.164 already on Conversation; SMS adapter |
| `close_session` | `end_session` |

Return JSON results on the **HTTP response** Vapi expects. Auth: Vapi secret header or shared HMAC; do not leave open.

Configure the hosted assistant with these tool names. Transient assistant JSON for laptop demo lives in `config/demo-assistant.json`.

### 7.3 Voice attach (`src/vapi_voice.rs`)

On voice Session with hours open:

1. Ensure AI Participant id (sqlite + `join_session`).
2. Inject timeline via `add_message(system|user, …, trigger_response=false)` as needed.
3. `vapi.attach_agent_for_participant(orch, caller_conn, ai_pid, options)`.
4. Subscribe events: transcripts → desk; `end` → session end policy.

On `take_over`: `mute_assistant()` then Orchestrator `take_over`. Do not `end()` Vapi if `ai.after_handoff=observe`.

If SMS/chat arrives during live voice: `add_message("user", body, true)`.

### 7.4 Phase 4 gate

- Fake chat: widget text Session round-trip AI Message with `kind=ai` distinct from customer.
- Tool test: POST tool-call `request_human` → pickup row/event.
- Mock Vapi voice (rvoip-vapi mock pattern): attach, participant ids differ.

Live `VAPI_API_KEY` is **not** required for CI.

---

## 8. Phase 5 — SIP inbound

`src/sip.rs` using `rvoip-sip` (`UnifiedCoordinator` / `SipAdapter` as in example 14 / Thelve ingress — **copy bind+advertise patterns from example 14**, not `RvoipApp` events).

Flow (PRD §17.1):

1. `INVITE` → CLI E.164, DID, optional correlation header allowlist.
2. `identity::resolve_ingress`.
3. If Conversation already has Active **voice** Session → reject busy (486) + optional `send_message` SMS.
4. `start_session(cid, Voice)`; `route_inbound_connection` / accept; customer Participant `Human`/`Customer`.
5. Hours closed: play greeting if cheap (`play_audio`), record voicemail to blob, `Message medium=audio`, hangup. No Vapi.
6. Hours open: Phase 4 voice attach.

Loopback: two `rvoip-sip` StreamPeers or existing in-crate example pattern on 127.0.0.1.

**Gate:** SIP loopback INVITE from a test UA → Conversation keyed by CLI → Vapi attach **or** fake attach → BYE persists Conversation.

---

## 9. Phase 6 — SMS adapter

`src/sms/mod.rs`:

```rust
#[async_trait]
trait SmsAdapter {
    async fn send(&self, tenant: &TenantId, to_e164: &str, body: &str) -> Result<String>;
}
```

- `fake.rs`: log + in-memory inbox; `POST /v1/test/sms/inbound` (dev only) injects inbound.
- `telnyx.rs`: behind feature; inbound webhook `/v1/sms/inbound` signature-checked.

Inbound: parse from/to/body → match E.164 → Message → if no live voice, Vapi Chat; if live voice, `add_message` on Vapi call.

Outbound: refuse if E.164 not on Conversation identities.

**Gate:** fake inbound SMS continues Conversation; outbound without prior identity → 409/403; with identity → fake send.

---

## 10. Phase 7 — Widget (chat + Talk)

`web/widget`:

- UI: Message, Talk, End (PRD §13.1). No “Call us”.
- Chat: UCTP text Session + `message.send`.
- Talk: `session.invite` medium `voice`; then connection/stream as `rvoip-websocket` media tests. Idle/end text Session when voice `session.started`.
- Cookie: durable `parley_vid` if no `visitor_id`.
- Config snippet: `data-parley-src`, public site key, UCTP WS URL.

Serve widget from `parley` static in demo.

**Gate:** Chrome against laptop server: chat then Talk, **same cid** in network tab UCTP envelopes. Customer connection id stable across Talk start (new Session allowed; cid must not change).

---

## 11. Phase 8 — Operator desk

`web/desk`:

- Login via bootstrap/operator.
- Nav: **Conversations** only.
- Inbox filters: live voice, waiting pickup, unread.
- Conversation page: kind/role chips, timeline, SMS composer, Pickup, End, Close, Download vCon.
- Pickup: `POST /v1/sessions/{sid}/participants` + UCTP voice Connection as operator (same WS stack, operator token).
- Technical details disclosure: transport, codec — collapsed.
- Live transcript from Vapi events / ASR if present.

**Gate:** two browsers: widget Talk + desk Pickup → `take_over` → AI muted, human hears (or fake audio path). Customer `connection_id` unchanged in `GET /sessions/{sid}/connections`.

---

## 12. Phase 9 — Hours, recording, vCon wrap

- `hours.rs`: weekly table + tz; `is_open(now)`.
- Recording: `start_recording` on voice Session start; system Participant `observer`; consent string once per Conversation (config). Pause/resume HTTP as PRD.
- Session vCon: subscribe `Event::VconReady`, copy bytes to `blob_dir`, index sqlite.
- Conversation close: `src/vcon_wrap.rs` JSON `{ "parley.vcon.conversation.v1": { "cid", "session_vcons": [...], "message_ids": [...] } }` if rvoip has no multi-session envelope. Unsigned default.

**Gate:** close Conversation → file exists, parties include customer, ai, human (if pickup), system.

---

## 13. Phase 10 — Pickup / return_to_ai polish

`src/pickup.rs`:

- One target: browser operators (all logged-in) and/or `sip_uri`.
- Ring timeout → AI continues or voicemail per config.
- Accept: establish operator Connection into **existing** voice Session; `orchestrator.take_over(sid, human_pid, Human)`; `mute_assistant`.
- Operator hangup: `return_to_ai` → `set_participant_role(ai, Agent)` + `unmute_assistant` (or re-attach if Vapi ended). Fire `participant.role_changed`.

**Gate:** unit test state machine; demo path in Phase 11.

---

## 14. Phase 11 — PRD §8 laptop demo + CI

`scripts/run-laptop-demo.sh`:

1. `parley` with fake SMS, fake or mock Vapi chat, `PARLEY_OPERATOR_BOOTSTRAP`, sqlite tempfile.
2. Print widget URL, desk URL, UCTP WS, SIP bind.
3. `tests/demo_script.rs` (or Playwright if you already have it; otherwise Rust+WS+HTTP is enough for CI):

| Step | Assert |
|---|---|
| Widget chat | One `cid`; AI participant `kind=ai` |
| Talk | New voice Session same `cid`; text Session idle/ended |
| Pickup (second token) | Human `agent`, AI `observer`; customer connection id unchanged |
| Fake SMS | Message on same `cid` |
| Close | vCon download 200 |

CI: `cargo test` + widget `npm test` if any. No live Vapi/Telnyx required.

Hosted DID/SMS is **not** this phase.

**v1 is done** when this gate is green and a reviewer can complete PRD §8.2 in their own words.

---

## 15. Observability (thread through phases 2–11)

`src/observe.rs`:

- `tracing` spans: `tenant_id`, `conversation_id`, `session_id`, `participant_id`, `connection_id`.
- Counters: `parley_conversations_opened`, `parley_sessions{medium}`, `parley_pickup_accept_ms`, `parley_handoffs`, `parley_sms_fail`, `parley_vcon_fail`.
- `/healthz`: sqlite, uctp bind, sip bind, blob dir writable. No secrets.

Never log message bodies/transcripts at `info`.

---

## 16. Auth matrix (implement in Phase 2, tighten in 3/8)

| Client | Credential | Can |
|---|---|---|
| Backend | Tenant bearer | Full `/v1` |
| Widget | Short-lived token | UCTP + that Conversation only |
| Operator | Cookie/session | Desk + pickup + tenant config |
| SIP | ACL / digest as rvoip config | Media only |
| Vapi tools | Shared secret | `/v1/vapi/tools` only |
| SMS inbound | Provider signature | `/v1/sms/inbound` only |

---

## 17. Test map

| Test | Phase | What it locks |
|---|---|---|
| `rvoip-core` participant_roles | 1 | UP-2 |
| `rvoip-vapi` mock attach ids | 1 | UP-1 |
| `rvoip-websocket` conversation_create | 1 | UP-3 |
| `api_conversations` | 2 | nouns, no `calls` |
| `identity_match` | 2 | e164/visitor continue |
| `uctp_create` | 3 | envelopes not SDP JSON |
| `vapi_chat_fake` | 4 | same AI pid on chat |
| `vapi_tools_request_human` | 4 | pickup event |
| `sip_loopback` | 5 | PSTN continue |
| `sms_fake` | 6 | Message medium=sms |
| `demo_script` (§8) | 11 | release gate |

---

## 18. Explicit non-work (stop and re-read PRD)

- `RvoipApp` conversation loop, `InboundCallAccepted` / `CallEstablished` in Parley.
- Vapi Twilio inbound SMS as the store.
- Harness ASR/TTS.
- Queues, CRM, skills, conference, MoQ, video, OIDC, Postgres (v1.1).
- Widget `{type:offer,sdp}` like example 14.
- Two-leg replace (Bridgefu).
- Probabilistic identity merge.
- Cold SMS.
- Second live voice Session / conference to dodge busy.

---

## 19. Suggested calendar (single implementer)

Not a promise; a packing order:

1. rvoip UP-2 → UP-1 → UP-3 (library PR).
2. Parley store + `/v1` + identity.
3. UCTP WS auth + text Session.
4. Vapi Chat fake + tools.
5. SIP loopback + Vapi voice mock.
6. SMS fake.
7. Widget chat+Talk.
8. Desk pickup.
9. Hours + vCon wrap.
10. §8 script in CI.

If staffing two people: A owns Phase 1 + 5 + 4 voice; B owns Phase 2–3, 4 chat, 6–8, 11. Integrate on `attach_agent_for_participant` and `take_over`.

---

## 20. First commands

```sh
# rvoip
cd ~/Developer/rvoip
git checkout -b parley/upstream
# implement Phase 1; run gates in §4.5
git rev-parse HEAD

# parley
cd ~/Developer/parley
# cargo init already implied; add Cargo.toml pin:
# rvoip-core = { git = "ssh://...", rev = "<sha>", package = "rvoip-core" }
# (exact git URL as you publish the branch)
```

When Phase 1 lands on crates.io, delete the git pin and use `=x.y.z` like Bridgefu.

---

## 21. Done

v1 is implemented when:

1. `parley/upstream` gates in §4.5 pass.
2. `tests/demo_script.rs` (or equivalent) passes PRD §8 on a laptop with fakes.
3. Public HTTP and UCTP speak only voip-3 nouns.
4. README can run the demo in 30 minutes.

Anything after that is PRD §23.2.
