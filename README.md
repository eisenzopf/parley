# Parley

Conversation server on rvoip voip-3 nouns. One durable **Conversation** across web chat, PSTN, browser voice, and SMS. HTTP `/v1` and the widget speak those nouns — never `call`, `leg`, `dialog`, or `bot`.

See [`PRD.md`](PRD.md) and [`IMPLEMENTATION_PLAN.md`](IMPLEMENTATION_PLAN.md).

Pins rvoip 0.3.12 at `ca7861af4c9920f6949422ae1f40507bf56ede97`, with additive conference patches in a separate `../rvoip-conference` checkout. Run `bash scripts/setup-rvoip.sh` to reproduce it. The original `../rvoip` checkout is preserved. The Vapi Rust SDK snapshot is included under `vendor/server-sdk-rust`; no separate SDK checkout is required. Run `node scripts/verify-conference-dependencies.mjs` to check the pinned dependency sources.

## Conference: one interface, more connections

The conference demo gives an external assistant and a human browser the experimental `conversation-control/1` UCTP interface. Rvoip connects the underlying systems; a canonical Conversation ties messages, participants, voice Sessions, and provider outcomes together.

Start with the [local runbook](docs/CONFERENCE_RUNBOOK.md), [wire profile](docs/UCTP_CONFERENCE_PROFILE.md), and [verification status](docs/CONFERENCE_IMPLEMENTATION_STATUS.md). The complete [demo plan](CONFERENCE_DEMO_PLAN.md) remains the release gate. Local Chrome/SIP audio handoff is verified; live carrier and provider rehearsal is still outstanding.

**The next connection could be yours.** [Contribute a connector or UCTP application](docs/CONTRIBUTING_CONNECTORS.md), include a reproducible example, and share what you connect. The contribution guide distinguishes working code from future integration ideas.

## Laptop demo (~30 minutes)

Requires Rust 1.91 (see `rust-toolchain.toml`), the sibling dependencies above, and live keys in `.env` (`VAPI_PRIVATE_KEY`, `TELNYX_TEST_API_KEY`, `CLOUDFLARE_KEY`). CI uses fakes; this script does not.

```sh
export PARLEY_API_SECRET=dev-only
export PARLEY_OPERATOR_BOOTSTRAP=bootstrap
./scripts/run-laptop-demo.sh
```

That prints:

| Surface | URL |
|---|---|
| HTTP `/v1` | http://127.0.0.1:8080 |
| Widget | http://127.0.0.1:8080/widget/ and https://parley.rudeless.ai/widget/ |
| Desk | http://127.0.0.1:8080/desk/ |
| UCTP WS | wss://parley.rudeless.ai/uctp |
| SIP | 127.0.0.1:5060 |
| SMS | +18058253932 |

Mint a widget token (or pass `PARLEY_API_SECRET` as `?token=` for a local demo):

```sh
curl -s -H "authorization: Bearer $PARLEY_API_SECRET" -H 'content-type: application/json' \
  -d '{"visitor_id":"usr_demo","origin":"https://parley.rudeless.ai"}' \
  http://127.0.0.1:8080/v1/widget/tokens
```

Open the widget with `?token=…`. It reads `GET /v1/public` for the tunnel UCTP URL. Message / Talk / End are UCTP envelopes. Chat and SMS go through the live Vapi assistant; SMS uses Telnyx. First boot creates the Parley assistant, messaging profile, and named tunnel once (`var/provision.json`).

Desk: login with bootstrap token `bootstrap` and an email, Refresh, Pickup. Pickup `take_over`s the live voice Session and mutes Vapi. `GET /v1/sessions/{sid}/connections` keeps `conn_customer_{sid}`.

The demo script binds `127.0.0.1:18080` (and 17443 / 15060) when 8080 / 7443 / 5060 are already taken. The tunnel still points at those binds.

CI covers the noun path without providers: `cargo test --test demo_script --test uctp_create --test desk_pickup_uctp`.

## Tests

```sh
./scripts/ci.sh
```

That runs JavaScript client tests and default `cargo test` (HTTP, identity, UCTP-first create, desk pickup, fake SMS/chat/tools, widget/desk contracts, conference, and demo script), then SIP loopback, Talk WebRTC, and browser checks when Playwright is available. It uses provider fixtures rather than live Vapi or Telnyx.

GitHub Actions (`.github/workflows/ci.yml`) checks out the pinned Rvoip baseline, applies the conference patch, and runs the same script.

## Health

`GET /healthz` reports sqlite, blob dir, UCTP bind, and SIP bind. It does not include secrets.
