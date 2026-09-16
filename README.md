# Parley

Conversation server on rvoip voip-3 nouns. One durable **Conversation** across web chat, PSTN, browser voice, and SMS. HTTP `/v1` and the widget speak those nouns — never `call`, `leg`, `dialog`, or `bot`.

See [`PRD.md`](PRD.md) and [`IMPLEMENTATION_PLAN.md`](IMPLEMENTATION_PLAN.md).

Pins rvoip `parley/upstream` via path (`../rvoip`). Merge-base: `68f94631`. Branch HEAD: `f4532ddf`.

## Laptop demo (~30 minutes)

Requires Rust 1.91 (see `rust-toolchain.toml`) and this repo next to `../rvoip`.

```sh
export PARLEY_API_SECRET=dev-only
export PARLEY_OPERATOR_BOOTSTRAP=bootstrap
export PARLEY_VAPI_CHAT=fake
./scripts/run-laptop-demo.sh
```

That prints:

| Surface | URL |
|---|---|
| HTTP `/v1` | http://127.0.0.1:8080 |
| Widget | http://127.0.0.1:8080/widget/ |
| Desk | http://127.0.0.1:8080/desk/ |
| UCTP WS | ws://127.0.0.1:7443 |
| SIP | 127.0.0.1:5060 |

Mint a widget token (or pass `PARLEY_API_SECRET` as `?token=` for a local demo):

```sh
curl -s -H 'authorization: Bearer dev-only' -H 'content-type: application/json' \
  -d '{"visitor_id":"usr_demo","origin":"http://127.0.0.1:8080"}' \
  http://127.0.0.1:8080/v1/widget/tokens
```

Open the widget with `?token=…&uctp=ws://127.0.0.1:7443`. Message / Talk / End are UCTP envelopes. Talk sends `connection.offer.substrate_setup` `{ kind: "websocket+webrtc", sdp }` (not a private `{type,sdp}` JSON). The widget stores a durable `parley_vid` cookie and can open a Conversation with `conversation.create` and a null cid (identity match on `visitor_id`).

Desk: login with bootstrap token `bootstrap` and an email, Refresh, Pickup. Pickup `take_over`s the live voice Session, then the desk sends an operator `session.invite` + `connection.offer` on `conn_operator_{sid}`. `GET /v1/sessions/{sid}/connections` keeps `conn_customer_{sid}`.

CI covers the same path without a browser: `cargo test --test demo_script --test uctp_create --test desk_pickup_uctp`.

## Tests

```sh
./scripts/ci.sh
```

That runs default `cargo test` (HTTP, identity, UCTP-first create, desk pickup, fake SMS/chat/tools, widget/desk contracts, demo script), then SIP loopback and Talk WebRTC. No live Vapi, Telnyx, or browser.

GitHub Actions (`.github/workflows/ci.yml`) checks out this repo next to `eisenzopf/rvoip` at branch `parley/upstream` and runs the same script.

## Health

`GET /healthz` reports sqlite, blob dir, UCTP bind, and SIP bind. It does not include secrets.
