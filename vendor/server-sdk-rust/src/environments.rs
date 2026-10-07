//! TypeScript: `export const VapiEnvironment = { Default: "https://api.vapi.ai" }`.

/// Default Vapi API origin. Same value as TypeScript `VapiEnvironment.Default`.
pub const VAPI_ENVIRONMENT_DEFAULT: &str = "https://api.vapi.ai";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VapiEnvironment;

impl VapiEnvironment {
    pub const DEFAULT: &'static str = VAPI_ENVIRONMENT_DEFAULT;
}
