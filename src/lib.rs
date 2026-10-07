//! Parley — conversation server on rvoip voip-3 nouns.

pub mod auth;
pub mod cloudflare_tunnel;
pub mod conference_network;
pub mod conference_voice;
pub mod conference_phone;
mod conference_phone_prompt;
pub mod config;
pub mod conversation;
pub mod error;
pub mod events;
pub mod hours;
pub mod http;
pub mod identity;
pub mod observe;
pub mod pickup;
pub mod provision;
pub mod recording;
pub mod runtime;
pub mod sip;
pub mod sms;
pub mod store;
pub mod tls;
#[cfg(feature = "uctp")]
pub mod uctp_commands;
pub mod uctp_host;
pub mod vapi_chat;
pub mod vapi_tools;
pub mod vapi_voice;
pub mod vcon_wrap;

pub use config::Config;
pub use error::{ApiError, Result};
pub use runtime::App;

#[cfg(feature = "uctp")]
mod uctp_observer;
