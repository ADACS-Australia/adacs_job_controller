mod app;
mod cluster;
mod config;
mod db;
mod http;
mod protocol;
mod utils;
mod websocket;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("russh=warn".parse().unwrap())
                .add_directive("sqlx=warn".parse().unwrap())
                .add_directive("tungstenite=warn".parse().unwrap())
                .add_directive("tokio_tungstenite=warn".parse().unwrap()),
        )
        .init();

    tracing::info!("ADACS Job Controller starting");
    tracing::debug!("Loading configuration and initializing components");

    let result = app::run().await;

    match &result {
        Ok(()) => tracing::info!("ADACS Job Controller shutdown complete"),
        Err(e) => tracing::error!("ADACS Job Controller terminated with error: {}", e),
    }

    result
}
