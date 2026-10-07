//! Process-wide rustls 0.23 crypto provider.
//!
//! Telnyx `rustls-ring` uses reqwest's `rustls-no-provider`. Cloudflare and the
//! Vapi client use rustls 0.23 via reqwest. rustls 0.23 has no implicit default
//! when compiled with `ring` only — install ring once before any HTTPS client.

use std::sync::Once;

static INSTALL: Once = Once::new();

/// Install [`rustls::crypto::ring`] as the process [`CryptoProvider`] if none is set.
pub fn ensure_rustls_ring() {
    INSTALL.call_once(|| {
        if rustls::crypto::CryptoProvider::get_default().is_some() {
            return;
        }
        rustls::crypto::ring::default_provider()
            .install_default()
            .expect("rustls ring CryptoProvider");
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn live_https_clients_construct() {
        super::ensure_rustls_ring();
        telnyx::Client::builder()
            .api_key("KEY0123")
            .build()
            .expect("telnyx client with rustls ring");
        vapi::VapiClient::new("sk-test").expect("vapi client with rustls ring");
    }
}
