//! Errors that match the TypeScript SDK’s `VapiError` / `VapiTimeoutError`.
//! The API token is never stored on these types and must never appear in `Display`.

use reqwest::StatusCode;
use serde_json::Value;
use std::fmt;
use std::time::Duration;

pub type Result<T> = std::result::Result<T, VapiError>;

/// Thrown when the API returns a non-success status (4xx / 5xx), or when the
/// client cannot complete a request.
///
/// TypeScript:
/// ```text
/// try { await client.calls.create(...) }
/// catch (err) {
///   if (err instanceof VapiError) {
///     console.log(err.statusCode);
///     console.log(err.body);
///   }
/// }
/// ```
#[derive(Debug)]
pub struct VapiError {
    /// HTTP status when the failure was an API response.
    pub status_code: Option<u16>,
    /// Parsed JSON body, if any. Never includes the bearer token.
    pub body: Option<Value>,
    kind: VapiErrorKind,
}

#[derive(Debug)]
enum VapiErrorKind {
    Api { message: String },
    Timeout,
    Transport { message: String },
    Decode { message: String },
    MissingToken,
    InvalidBaseUrl,
}

impl VapiError {
    pub fn status_code(&self) -> Option<u16> {
        self.status_code
    }

    pub fn body(&self) -> Option<&Value> {
        self.body.as_ref()
    }

    pub fn is_timeout(&self) -> bool {
        matches!(self.kind, VapiErrorKind::Timeout)
    }

    pub fn is_unauthorized(&self) -> bool {
        matches!(self.status_code, Some(401 | 403))
    }

    pub(crate) fn missing_token() -> Self {
        Self {
            status_code: None,
            body: None,
            kind: VapiErrorKind::MissingToken,
        }
    }

    pub(crate) fn invalid_base_url() -> Self {
        Self {
            status_code: None,
            body: None,
            kind: VapiErrorKind::InvalidBaseUrl,
        }
    }

    pub(crate) fn timeout() -> Self {
        Self {
            status_code: None,
            body: None,
            kind: VapiErrorKind::Timeout,
        }
    }

    pub(crate) fn decode(message: impl Into<String>) -> Self {
        Self {
            status_code: None,
            body: None,
            kind: VapiErrorKind::Decode {
                message: message.into(),
            },
        }
    }

    pub(crate) fn from_status(status: StatusCode, body_text: &str) -> Self {
        let body = serde_json::from_str::<Value>(body_text).ok();
        let message = body
            .as_ref()
            .and_then(|v| {
                v.get("message")
                    .or_else(|| v.get("error"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_default();
        Self {
            status_code: Some(status.as_u16()),
            body,
            kind: VapiErrorKind::Api { message },
        }
    }

    pub(crate) fn from_reqwest(err: reqwest::Error) -> Self {
        if err.is_timeout() {
            return Self::timeout();
        }
        Self {
            status_code: err.status().map(|s| s.as_u16()),
            body: None,
            kind: VapiErrorKind::Transport {
                message: err.without_url().to_string(),
            },
        }
    }
}

impl fmt::Display for VapiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            VapiErrorKind::Api { message } => {
                if !message.is_empty() {
                    writeln!(f, "{message}")?;
                }
                if let Some(code) = self.status_code {
                    writeln!(f, "Status code: {code}")?;
                }
                if let Some(body) = &self.body {
                    write!(f, "Body: {body}")?;
                }
                Ok(())
            }
            VapiErrorKind::Timeout => write!(f, "request timed out"),
            VapiErrorKind::Transport { message } => write!(f, "{message}"),
            VapiErrorKind::Decode { message } => write!(f, "invalid Vapi JSON: {message}"),
            VapiErrorKind::MissingToken => {
                write!(f, "missing token: set VAPI_PRIVATE_KEY or VAPI_API_KEY")
            }
            VapiErrorKind::InvalidBaseUrl => write!(
                f,
                "Vapi API base URL must be HTTPS (or loopback HTTP for tests)"
            ),
        }
    }
}

impl std::error::Error for VapiError {}

impl From<reqwest::Error> for VapiError {
    fn from(err: reqwest::Error) -> Self {
        Self::from_reqwest(err)
    }
}

impl From<url::ParseError> for VapiError {
    fn from(_: url::ParseError) -> Self {
        Self::invalid_base_url()
    }
}

impl From<serde_json::Error> for VapiError {
    fn from(err: serde_json::Error) -> Self {
        Self::decode(err.to_string())
    }
}

/// Alias matching TypeScript `VapiTimeoutError`.
pub type VapiTimeoutError = VapiError;

/// Per-request overrides. TypeScript: `{ timeoutInSeconds, maxRetries }`.
#[derive(Debug, Clone, Default)]
pub struct RequestOptions {
    pub timeout: Option<Duration>,
    pub max_retries: Option<u32>,
}

impl RequestOptions {
    pub fn timeout_in_seconds(secs: u64) -> Self {
        Self {
            timeout: Some(Duration::from_secs(secs)),
            max_retries: None,
        }
    }
}
