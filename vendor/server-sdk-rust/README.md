# Vapi Rust Library

The Vapi Rust library provides convenient access to the Vapi API from Rust.

This crate is **hand-written**. The TypeScript, Python, Go, Java, Ruby, C#, PHP,
and Swift server SDKs are Fern-generated; Fern does not ship a Rust generator we
want to maintain. Resource names, constructor options, retries, and errors
follow [`@vapi-ai/server-sdk`](https://github.com/VapiAI/server-sdk-typescript).
Method names are snake_case; ids are `&str` rather than `{ id: string }` objects.

This is the crate to land at [`VapiAI/server-sdk-rust`](https://github.com/VapiAI/server-sdk-rust)
(crates.io name `vapi`, matching Python’s `from vapi import Vapi`). Until that
org repo exists, develop against this directory and publish from your Vapi
GitHub account.

## Installation

Local path (Parley and other siblings):

```toml
vapi = { path = "../server-sdk-rust" }
```

After it is on GitHub / crates.io:

```toml
vapi = "0.1"
# or
vapi = { git = "https://github.com/VapiAI/server-sdk-rust" }
```

TLS is **rustls** by default (`native-tls` is optional). Every request sets
`User-Agent: vapi/<version>` — `api.vapi.ai` sits behind Cloudflare and rejects
clients with no User-Agent.

## Usage

Instantiate and use the client with the following:

```rust
use vapi::{CreateCallRequest, VapiClient};

# async fn demo() -> vapi::Result<()> {
let client = VapiClient::new("YOUR_TOKEN")?;
client.calls().create(CreateCallRequest::default()).await?;
# Ok(())
# }
```

TypeScript equivalent:

```typescript
import { VapiClient } from "@vapi-ai/server-sdk";

const client = new VapiClient({ token: "YOUR_TOKEN" });
await client.calls.create();
```

`VapiClient::from_env()` also reads `VAPI_PRIVATE_KEY` or `VAPI_API_KEY`.

## Request and response types

The SDK exports request and response types at the crate root and under
`vapi::types`. Construct requests with struct update syntax:

```rust
use vapi::CreateChatRequest;

let request = CreateChatRequest {
    assistant_id: Some("asst_…".into()),
    input: Some("what are your hours?".into()),
    ..Default::default()
};
```

Unknown JSON fields deserialize into `extra: Value` so new API fields do not
break the client.

## Exception handling

When the API returns a non-success status code (4xx or 5xx), a [`VapiError`] is
returned. Same fields as TypeScript: `status_code` and `body`.

```rust
use vapi::{VapiClient, VapiError};

# async fn demo() -> Result<(), Box<dyn std::error::Error>> {
let client = VapiClient::new("YOUR_TOKEN")?;
match client.assistants().list().await {
    Ok(list) => println!("{} assistants", list.len()),
    Err(err) => {
        println!("{:?}", err.status_code);
        println!("{:?}", err.body);
        let _ : VapiError = err;
    }
}
# Ok(())
# }
```

The bearer token is never stored on `VapiError` and never appears in `Debug` for
`VapiClient`.

Timeouts are `VapiError::is_timeout()` (TypeScript `VapiTimeoutError`).

## Retries

The SDK retries with exponential backoff. A request is retried while it is
retriable and the attempt count has not exceeded the retry limit (**default: 2**).

A request is retriable when any of the following HTTP status codes is returned:

- [408](https://developer.mozilla.org/en-US/docs/Web/HTTP/Status/408) (Timeout)
- [429](https://developer.mozilla.org/en-US/docs/Web/HTTP/Status/429) (Too Many Requests)
- [5XX](https://developer.mozilla.org/en-US/docs/Web/HTTP/Status/500) (Internal Server Errors)

```rust
# fn demo() -> vapi::Result<()> {
let client = vapi::VapiClient::builder()
    .token("YOUR_TOKEN")
    .max_retries(0) // TypeScript: { maxRetries: 0 }
    .build()?;
# let _ = client;
# Ok(())
# }
```

## Timeouts

The SDK defaults to a **60 second** timeout. TypeScript: `timeoutInSeconds`.

```rust
# fn demo() -> vapi::Result<()> {
let client = vapi::VapiClient::builder()
    .token("YOUR_TOKEN")
    .timeout_in_seconds(30)
    .build()?;
# let _ = client;
# Ok(())
# }
```

## Resources

| Rust | TypeScript | HTTP |
|---|---|---|
| `client.assistants()` | `client.assistants` | `/assistant` |
| `client.squads()` | `client.squads` | `/squad` |
| `client.calls()` | `client.calls` | `/call` |
| `client.chats()` | `client.chats` | `/chat` |
| `client.campaigns()` | `client.campaigns` | `/campaign` |
| `client.sessions()` | `client.sessions` | `/session` |
| `client.phone_numbers()` | `client.phoneNumbers` | `/phone-number` |
| `client.tools()` | `client.tools` | `/tool` |
| `client.files()` | `client.files` | `/file` |
| `client.structured_outputs()` | `client.structuredOutputs` | `/structured-output` |
| `client.simulations()` | `client.simulations` | `/simulation` |
| `client.analytics()` | `client.analytics` | `/analytics` |
| `client.eval()` | `client.eval` | `/eval` |

Assistants, chats, calls, phone numbers, tools, squads, and sessions are typed.
The rest return `serde_json::Value` until we add structs. Anything else:

```rust
# async fn demo(client: vapi::VapiClient) -> vapi::Result<()> {
let value = client.get("/assistant").await?;
# let _ = value;
# Ok(())
# }
```

## Environment

TypeScript `VapiEnvironment.Default` is `https://api.vapi.ai`. Same constant:
`vapi::VapiEnvironment::DEFAULT`. Production bases must be HTTPS. Loopback
`http://` is allowed for tests.

## Contributing

This library is **not** generated. Additions here will not be overwritten by
Fern. See [`CONTRIBUTING.md`](CONTRIBUTING.md).

## License

MIT, Vapi Labs Inc.
