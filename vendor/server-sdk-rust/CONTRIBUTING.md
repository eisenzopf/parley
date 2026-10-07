# Contributing

Thanks for contributing. This crate is the candidate
[`VapiAI/server-sdk-rust`](https://github.com/VapiAI/server-sdk-rust). That
repo does not exist yet; the other official server SDKs live at
`VapiAI/server-sdk-<language>`.

## Conventions (match TypeScript, stay rusty)

Follow [`@vapi-ai/server-sdk`](https://github.com/VapiAI/server-sdk-typescript):

| TypeScript | Rust |
|---|---|
| `new VapiClient({ token })` | `VapiClient::new(token)` |
| `client.calls.create()` | `client.calls().create(...).await` |
| `client.phoneNumbers` | `client.phone_numbers()` |
| `client.assistants.get({ id })` | `client.assistants().get(id)` |
| `VapiError.statusCode` / `.body` | `VapiError.status_code` / `.body` |
| `timeoutInSeconds` (default 60) | `timeout_in_seconds` / `Duration` |
| `maxRetries` (default 2; 408/429/5xx) | `max_retries` |
| `VapiEnvironment.Default` | `VapiEnvironment::DEFAULT` |
| `User-Agent: @vapi-ai/server-sdk/<ver>` | `User-Agent: vapi/<ver>` |

Do **not** copy Fern artifacts such as
`assistantControllerValidateBackgroundSoundUrl`. Use snake_case names a Rust
caller would write.

## Rules

- **TLS on by default** (rustls). Never ship `reqwest` without TLS.
- **User-Agent on every request.** Cloudflare in front of `api.vapi.ai` returns
  1010 without one.
- **Never log the API token.** `Debug` for `VapiClient` / `ApiToken` must
  redact. Do not put the token on `VapiError`.
- **Forward-compatible types.** Unknown keys go in `#[serde(flatten)] extra`.
- **List endpoints** may return a bare array or `{ results \| data \| items }`.
- **HTTPS production base.** `http://` only for loopback tests.
- README sections match the TypeScript library: Usage, types, errors, retries,
  timeouts, contributing.

This crate is **not** Fern-generated. Direct PRs here are welcome.

## Tests

```sh
cargo test
```

Live API tests are not in CI. Use `VAPI_PRIVATE_KEY` locally — never commit or
print a key.

## Publishing

Push this repo to GitHub as `server-sdk-rust` (Vapi account, not a personal
fork of Parley). Description: “The official Rust SDK for accessing Vapi's API”.
Then publish `vapi` to crates.io. Parley keeps a path dep until that lands.
