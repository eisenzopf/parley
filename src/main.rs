use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("parley=info".parse()?))
        .init();

    let config = parley::Config::load()?;
    let store = parley::store::Store::open(&config)?;
    let app = parley::App::new(config, store)?;
    app.serve().await?;
    Ok(())
}
