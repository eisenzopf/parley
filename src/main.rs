use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    parley::tls::ensure_rustls_ring();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("parley=info".parse()?))
        .init();

    let mut config = parley::Config::load()?;
    if config.tunnel_enabled() {
        match parley::cloudflare_tunnel::ensure_and_run(&config).await {
            Ok(bindings) => {
                config.vapi_public_base = bindings.public_base.clone();
                config.uctp_public_ws = bindings.uctp_ws_url.clone();
                if config.public_hostname.is_empty() {
                    config.public_hostname = bindings.hostname.clone();
                }
                tracing::info!(
                    hostname = %bindings.hostname,
                    http = %bindings.public_base,
                    uctp = %bindings.uctp_ws_url,
                    "cloudflare tunnel ready"
                );
            }
            Err(e) => return Err(format!("cloudflare tunnel: {e}").into()),
        }
    }
    parley::provision::ensure(&mut config).await?;
    if config.vapi_chat_mode != "fake" {
        if config.vapi_configured() && config.vapi_assistant_id.is_empty() {
            return Err(
                "VAPI_PRIVATE_KEY is set but no Parley assistant id was provisioned".into(),
            );
        }
        if config.telnyx_configured() && config.telnyx_from.is_empty() {
            return Err("TELNYX_TEST_API_KEY is set but no SMS number was provisioned".into());
        }
    }
    let store = parley::store::Store::open(&config)?;
    let app = parley::App::new(config, store)?;
    app.serve().await?;
    Ok(())
}
