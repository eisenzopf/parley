use crate::environments::VAPI_ENVIRONMENT_DEFAULT;
use crate::error::{Result, VapiError};
use crate::list::decode_list;
use crate::types::*;
use reqwest::{Client as Http, Method, StatusCode};
use serde::Serialize;
use serde_json::Value;
use std::fmt;
use std::time::Duration;
use url::Url;

const ENV_PRIVATE: &str = "VAPI_PRIVATE_KEY";
const ENV_API: &str = "VAPI_API_KEY";
const ENV_BASE: &str = "VAPI_API_BASE";
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
const DEFAULT_MAX_RETRIES: u32 = 2;

#[derive(Clone)]
pub struct ApiToken(String);

impl ApiToken {
    pub fn new(token: impl Into<String>) -> Self {
        Self(token.into())
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiToken(<redacted>)")
    }
}

/// Client for the Vapi API. TypeScript: `new VapiClient({ token })`.
///
/// ```no_run
/// use vapi::VapiClient;
/// # fn main() -> vapi::Result<()> {
/// let client = VapiClient::new("YOUR_TOKEN")?;
/// # let _ = client;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct VapiClient {
    http: Http,
    base: Url,
    token: ApiToken,
    max_retries: u32,
}

impl fmt::Debug for VapiClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VapiClient")
            .field("base", &self.base)
            .field("token", &self.token)
            .field("max_retries", &self.max_retries)
            .finish()
    }
}

impl VapiClient {
    /// TypeScript: `new VapiClient({ token: "YOUR_TOKEN" })`.
    pub fn new(token: impl Into<String>) -> Result<Self> {
        VapiClientBuilder::new().token(token).build()
    }

    /// Reads `VAPI_PRIVATE_KEY`, then `VAPI_API_KEY`. Extra to the TS SDK.
    pub fn from_env() -> Result<Self> {
        VapiClientBuilder::from_env()?.build()
    }

    pub fn builder() -> VapiClientBuilder {
        VapiClientBuilder::new()
    }

    /// TypeScript: `client.assistants`.
    pub fn assistants(&self) -> AssistantsClient<'_> {
        AssistantsClient { client: self }
    }

    /// TypeScript: `client.squads`.
    pub fn squads(&self) -> SquadsClient<'_> {
        SquadsClient { client: self }
    }

    /// TypeScript: `client.calls`.
    pub fn calls(&self) -> CallsClient<'_> {
        CallsClient { client: self }
    }

    /// TypeScript: `client.chats`.
    pub fn chats(&self) -> ChatsClient<'_> {
        ChatsClient { client: self }
    }

    /// TypeScript: `client.campaigns`.
    pub fn campaigns(&self) -> ResourceClient<'_> {
        ResourceClient {
            client: self,
            path: "/campaign",
        }
    }

    /// TypeScript: `client.sessions`.
    pub fn sessions(&self) -> SessionsClient<'_> {
        SessionsClient { client: self }
    }

    /// TypeScript: `client.phoneNumbers`.
    pub fn phone_numbers(&self) -> PhoneNumbersClient<'_> {
        PhoneNumbersClient { client: self }
    }

    /// TypeScript: `client.tools`.
    pub fn tools(&self) -> ToolsClient<'_> {
        ToolsClient { client: self }
    }

    /// TypeScript: `client.files`.
    pub fn files(&self) -> ResourceClient<'_> {
        ResourceClient {
            client: self,
            path: "/file",
        }
    }

    /// TypeScript: `client.knowledgeBasesV2`.
    pub fn knowledge_bases_v2(&self) -> ResourceClient<'_> {
        ResourceClient {
            client: self,
            path: "/knowledge-base",
        }
    }

    /// TypeScript: `client.structuredOutputs`.
    pub fn structured_outputs(&self) -> ResourceClient<'_> {
        ResourceClient {
            client: self,
            path: "/structured-output",
        }
    }

    /// TypeScript: `client.simulations`.
    pub fn simulations(&self) -> ResourceClient<'_> {
        ResourceClient {
            client: self,
            path: "/simulation",
        }
    }

    /// TypeScript: `client.analytics`.
    pub fn analytics(&self) -> ResourceClient<'_> {
        ResourceClient {
            client: self,
            path: "/analytics",
        }
    }

    /// TypeScript: `client.eval`.
    pub fn eval(&self) -> ResourceClient<'_> {
        ResourceClient {
            client: self,
            path: "/eval",
        }
    }

    /// Passthrough for endpoints not yet typed. TypeScript: `client.fetch(...)`.
    pub async fn get(&self, path: &str) -> Result<Value> {
        self.request(Method::GET, path, None).await
    }

    pub async fn post(&self, path: &str, body: &Value) -> Result<Value> {
        self.request(Method::POST, path, Some(body)).await
    }

    pub async fn patch(&self, path: &str, body: &Value) -> Result<Value> {
        self.request(Method::PATCH, path, Some(body)).await
    }

    pub async fn delete(&self, path: &str) -> Result<Value> {
        self.request(Method::DELETE, path, None).await
    }

    pub(crate) async fn send_json<T, B>(&self, method: Method, path: &str, body: Option<&B>) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
        B: Serialize,
    {
        let value = match body {
            Some(b) => {
                let json = serde_json::to_value(b)?;
                self.request(method, path, Some(&json)).await?
            }
            None => self.request(method, path, None).await?,
        };
        Ok(serde_json::from_value(value)?)
    }

    pub(crate) async fn send_list<T>(&self, path: &str) -> Result<Vec<T>>
    where
        T: serde::de::DeserializeOwned,
    {
        let value = self.request(Method::GET, path, None).await?;
        Ok(decode_list(value)?)
    }

    async fn request(&self, method: Method, path: &str, body: Option<&Value>) -> Result<Value> {
        let url = join_path(&self.base, path)?;
        let mut attempt = 0;
        loop {
            let mut req = self
                .http
                .request(method.clone(), url.clone())
                .bearer_auth(self.token.expose())
                .header("Accept", "application/json")
                .header("X-Fern-Language", "Rust")
                .header("X-Fern-SDK-Name", "vapi")
                .header("X-Fern-SDK-Version", env!("CARGO_PKG_VERSION"));
            if let Some(body) = body {
                req = req.json(body);
            }
            let response = match req.send().await {
                Ok(resp) => resp,
                Err(err) if err.is_timeout() => return Err(VapiError::timeout()),
                Err(err) => return Err(VapiError::from(err)),
            };
            let status = response.status();
            let retry_after = retry_after_secs(&response);
            let text = response.text().await.unwrap_or_default();
            if status.is_success() {
                if text.trim().is_empty() {
                    return Ok(Value::Null);
                }
                return Ok(serde_json::from_str(&sanitize_json(&text))?);
            }
            if attempt < self.max_retries && is_retryable(status) {
                attempt += 1;
                let wait = retry_after
                    .map(Duration::from_secs)
                    .unwrap_or_else(|| Duration::from_millis(200 * 2u64.pow(attempt)));
                tokio::time::sleep(wait).await;
                continue;
            }
            return Err(VapiError::from_status(status, &text));
        }
    }
}

/// TypeScript: `new VapiClient({ token, timeoutInSeconds, maxRetries, baseUrl })`.
pub struct VapiClientBuilder {
    token: Option<String>,
    base: String,
    timeout: Duration,
    max_retries: u32,
}

impl VapiClientBuilder {
    pub fn new() -> Self {
        Self {
            token: None,
            base: VAPI_ENVIRONMENT_DEFAULT.into(),
            timeout: DEFAULT_TIMEOUT,
            max_retries: DEFAULT_MAX_RETRIES,
        }
    }

    pub fn from_env() -> Result<Self> {
        let token = std::env::var(ENV_PRIVATE)
            .ok()
            .or_else(|| std::env::var(ENV_API).ok())
            .and_then(normalize_secret)
            .ok_or_else(VapiError::missing_token)?;
        let mut builder = Self::new().token(token);
        if let Some(base) = std::env::var(ENV_BASE).ok().and_then(normalize_secret) {
            builder = builder.base_url(base);
        }
        Ok(builder)
    }

    pub fn token(mut self, token: impl Into<String>) -> Self {
        self.token = normalize_secret(token.into());
        self
    }

    pub fn base_url(mut self, base: impl Into<String>) -> Self {
        self.base = base.into();
        self
    }

    pub fn environment(self, environment: impl Into<String>) -> Self {
        self.base_url(environment)
    }

    /// TypeScript: `timeoutInSeconds`. Default 60.
    pub fn timeout_in_seconds(self, secs: u64) -> Self {
        self.timeout(Duration::from_secs(secs))
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// TypeScript: `maxRetries`. Default 2. `0` disables retries.
    pub fn max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    pub fn build(self) -> Result<VapiClient> {
        let token = self
            .token
            .filter(|s| !s.is_empty())
            .ok_or_else(VapiError::missing_token)?;
        let base = Url::parse(&self.base)?;
        validate_base(&base)?;
        ensure_rustls_ring();
        let http = Http::builder()
            .timeout(self.timeout)
            .user_agent(concat!("vapi/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(VapiClient {
            http,
            base,
            token: ApiToken::new(token),
            max_retries: self.max_retries,
        })
    }
}

impl Default for VapiClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// JSON resource with list/get/create/update/delete. Used for TS resources we
/// have not given dedicated structs yet.
pub struct ResourceClient<'a> {
    client: &'a VapiClient,
    path: &'static str,
}

impl ResourceClient<'_> {
    pub async fn list(&self) -> Result<Value> {
        self.client.get(self.path).await
    }

    pub async fn get(&self, id: &str) -> Result<Value> {
        self.client.get(&format!("{}/{id}", self.path)).await
    }

    pub async fn create(&self, body: &Value) -> Result<Value> {
        self.client.post(self.path, body).await
    }

    pub async fn update(&self, id: &str, body: &Value) -> Result<Value> {
        self.client.patch(&format!("{}/{id}", self.path), body).await
    }

    pub async fn delete(&self, id: &str) -> Result<Value> {
        self.client.delete(&format!("{}/{id}", self.path)).await
    }
}

/// TypeScript `AssistantsClient`. `get`/`delete` take `&str` ids, not `{ id }`.
pub struct AssistantsClient<'a> {
    client: &'a VapiClient,
}

impl AssistantsClient<'_> {
    /// ```ignore
    /// await client.assistants.list()
    /// ```
    pub async fn list(&self) -> Result<Vec<Assistant>> {
        self.client.send_list("/assistant").await
    }

    pub async fn get(&self, id: &str) -> Result<Assistant> {
        self.client
            .send_json::<Assistant, Value>(Method::GET, &format!("/assistant/{id}"), None)
            .await
    }

    pub async fn create(&self, body: CreateAssistantRequest) -> Result<Assistant> {
        self.client.send_json(Method::POST, "/assistant", Some(&body)).await
    }

    pub async fn create_json(&self, body: &Value) -> Result<Assistant> {
        self.client.send_json(Method::POST, "/assistant", Some(body)).await
    }

    pub async fn update(&self, id: &str, body: &Value) -> Result<Assistant> {
        self.client
            .send_json(Method::PATCH, &format!("/assistant/{id}"), Some(body))
            .await
    }

    pub async fn delete(&self, id: &str) -> Result<Assistant> {
        self.client
            .send_json::<Assistant, Value>(Method::DELETE, &format!("/assistant/{id}"), None)
            .await
    }
}

/// TypeScript `ChatsClient`.
pub struct ChatsClient<'a> {
    client: &'a VapiClient,
}

impl ChatsClient<'_> {
    pub async fn list(&self) -> Result<Vec<Chat>> {
        self.client.send_list("/chat").await
    }

    /// ```ignore
    /// await client.chats.create({ input: "input" })
    /// ```
    pub async fn create(&self, body: CreateChatRequest) -> Result<Chat> {
        self.client.send_json(Method::POST, "/chat", Some(&body)).await
    }

    pub async fn get(&self, id: &str) -> Result<Chat> {
        self.client
            .send_json::<Chat, Value>(Method::GET, &format!("/chat/{id}"), None)
            .await
    }

    pub async fn delete(&self, id: &str) -> Result<Chat> {
        self.client
            .send_json::<Chat, Value>(Method::DELETE, &format!("/chat/{id}"), None)
            .await
    }

    /// TypeScript: `client.chats.createResponse`.
    pub async fn create_response(&self, body: &Value) -> Result<Value> {
        self.client.post("/chat/responses", body).await
    }
}

/// TypeScript `CallsClient`.
pub struct CallsClient<'a> {
    client: &'a VapiClient,
}

impl CallsClient<'_> {
    pub async fn list(&self, query: ListCallsRequest) -> Result<Vec<Call>> {
        let mut path = format!("/call?limit={}", query.limit.unwrap_or(50));
        if let Some(id) = query.assistant_id {
            path.push_str("&assistantId=");
            path.push_str(&urlencoding_lite(&id));
        }
        self.client.send_list(&path).await
    }

    pub async fn get(&self, id: &str) -> Result<Call> {
        self.client
            .send_json::<Call, Value>(Method::GET, &format!("/call/{id}"), None)
            .await
    }

    /// ```ignore
    /// await client.calls.create()
    /// ```
    pub async fn create(&self, body: CreateCallRequest) -> Result<Call> {
        self.client.send_json(Method::POST, "/call", Some(&body)).await
    }

    pub async fn update(&self, id: &str, body: &Value) -> Result<Call> {
        self.client
            .send_json(Method::PATCH, &format!("/call/{id}"), Some(body))
            .await
    }

    pub async fn delete(&self, id: &str) -> Result<Call> {
        self.client
            .send_json::<Call, Value>(Method::DELETE, &format!("/call/{id}"), None)
            .await
    }

    /// Convenience for `update` with `{ "status": "ended" }`.
    pub async fn end(&self, id: &str) -> Result<Value> {
        self.client
            .patch(&format!("/call/{id}"), &serde_json::json!({ "status": "ended" }))
            .await
    }
}

/// TypeScript `PhoneNumbersClient`.
pub struct PhoneNumbersClient<'a> {
    client: &'a VapiClient,
}

impl PhoneNumbersClient<'_> {
    pub async fn list(&self) -> Result<Vec<PhoneNumber>> {
        self.client.send_list("/phone-number").await
    }

    pub async fn get(&self, id: &str) -> Result<PhoneNumber> {
        self.client
            .send_json::<PhoneNumber, Value>(Method::GET, &format!("/phone-number/{id}"), None)
            .await
    }

    pub async fn update(&self, id: &str, body: UpdatePhoneNumberRequest) -> Result<PhoneNumber> {
        self.client
            .send_json(Method::PATCH, &format!("/phone-number/{id}"), Some(&body))
            .await
    }
}

/// TypeScript `ToolsClient`.
pub struct ToolsClient<'a> {
    client: &'a VapiClient,
}

impl ToolsClient<'_> {
    pub async fn list(&self) -> Result<Vec<Tool>> {
        self.client.send_list("/tool").await
    }

    pub async fn get(&self, id: &str) -> Result<Tool> {
        self.client
            .send_json::<Tool, Value>(Method::GET, &format!("/tool/{id}"), None)
            .await
    }

    pub async fn create(&self, body: &Value) -> Result<Tool> {
        self.client.send_json(Method::POST, "/tool", Some(body)).await
    }

    pub async fn delete(&self, id: &str) -> Result<Value> {
        self.client.delete(&format!("/tool/{id}")).await
    }
}

/// TypeScript `SquadsClient`.
pub struct SquadsClient<'a> {
    client: &'a VapiClient,
}

impl SquadsClient<'_> {
    pub async fn list(&self) -> Result<Vec<Squad>> {
        self.client.send_list("/squad").await
    }

    pub async fn get(&self, id: &str) -> Result<Squad> {
        self.client
            .send_json::<Squad, Value>(Method::GET, &format!("/squad/{id}"), None)
            .await
    }

    pub async fn create(&self, body: &Value) -> Result<Squad> {
        self.client.send_json(Method::POST, "/squad", Some(body)).await
    }

    pub async fn update(&self, id: &str, body: &Value) -> Result<Squad> {
        self.client
            .send_json(Method::PATCH, &format!("/squad/{id}"), Some(body))
            .await
    }

    pub async fn delete(&self, id: &str) -> Result<Value> {
        self.client.delete(&format!("/squad/{id}")).await
    }
}

/// TypeScript `SessionsClient`.
pub struct SessionsClient<'a> {
    client: &'a VapiClient,
}

impl SessionsClient<'_> {
    pub async fn list(&self) -> Result<Vec<Session>> {
        self.client.send_list("/session?limit=50").await
    }

    pub async fn get(&self, id: &str) -> Result<Session> {
        self.client
            .send_json::<Session, Value>(Method::GET, &format!("/session/{id}"), None)
            .await
    }

    pub async fn create(&self, body: &Value) -> Result<Session> {
        self.client.send_json(Method::POST, "/session", Some(body)).await
    }

    pub async fn delete(&self, id: &str) -> Result<Value> {
        self.client.delete(&format!("/session/{id}")).await
    }
}

fn join_path(base: &Url, path: &str) -> Result<Url> {
    let path = path.trim_start_matches('/');
    Ok(base.join(path)?)
}

fn validate_base(url: &Url) -> Result<()> {
    match url.scheme() {
        "https" => Ok(()),
        "http" if is_loopback(url) => Ok(()),
        _ => Err(VapiError::invalid_base_url()),
    }
}

fn is_loopback(url: &Url) -> bool {
    matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "::1"))
}

fn is_retryable(status: StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 429) || status.is_server_error()
}

fn retry_after_secs(response: &reqwest::Response) -> Option<u64> {
    response
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
}

fn sanitize_json(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_control() && c != '\n' && c != '\r' && c != '\t' {
                ' '
            } else {
                c
            }
        })
        .collect()
}

fn normalize_secret(raw: String) -> Option<String> {
    let s = raw.trim().trim_matches('"').trim_matches('\'').trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

fn ensure_rustls_ring() {
    static INSTALL: std::sync::Once = std::sync::Once::new();
    INSTALL.call_once(|| {
        if rustls::crypto::CryptoProvider::get_default().is_some() {
            return;
        }
        rustls::crypto::ring::default_provider()
            .install_default()
            .expect("rustls ring CryptoProvider");
    });
}

fn urlencoding_lite(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_token() {
        let client = VapiClient::builder()
            .token("sk-secret-value")
            .base_url("http://127.0.0.1:9")
            .build()
            .unwrap();
        let debug = format!("{client:?}");
        assert!(!debug.contains("sk-secret-value"));
        assert!(debug.contains("<redacted>"));
    }

    #[test]
    fn rejects_cleartext_remote_base() {
        let err = VapiClient::builder()
            .token("sk")
            .base_url("http://api.vapi.ai")
            .build()
            .unwrap_err();
        assert!(err.status_code().is_none());
        assert!(format!("{err}").contains("HTTPS"));
    }
}
