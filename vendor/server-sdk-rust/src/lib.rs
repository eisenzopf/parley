//! The Vapi Rust library provides convenient access to the Vapi API from Rust.
//!
//! Resource names match [`@vapi-ai/server-sdk`](https://github.com/VapiAI/server-sdk-typescript).
//! Method names are snake_case; path ids are `&str` instead of `{ id }` objects.
//! Unknown JSON fields land in `extra` so spec additions do not break decode.
//!
//! # Usage
//!
//! ```no_run
//! use vapi::{CreateCallRequest, VapiClient};
//!
//! # async fn demo() -> vapi::Result<()> {
//! let client = VapiClient::new("YOUR_TOKEN")?;
//! client.calls().create(CreateCallRequest::default()).await?;
//! # Ok(())
//! # }
//! ```

mod client;
mod environments;
mod error;
mod list;
pub mod types;

pub use client::{
    ApiToken, AssistantsClient, CallsClient, ChatsClient, PhoneNumbersClient, ResourceClient,
    SessionsClient, SquadsClient, ToolsClient, VapiClient, VapiClientBuilder,
};
pub use environments::{VapiEnvironment, VAPI_ENVIRONMENT_DEFAULT};
pub use error::{RequestOptions, Result, VapiError, VapiTimeoutError};
pub use types::{
    Assistant, Call, Chat, ChatOutput, CreateAssistantRequest, CreateCallRequest, CreateChatRequest,
    ListCallsRequest, PhoneNumber, Session, Squad, Tool, UpdatePhoneNumberRequest,
};
