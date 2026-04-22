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
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    app::run().await
}
