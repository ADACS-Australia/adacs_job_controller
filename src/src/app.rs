use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use sea_orm_migration::MigratorTrait;
use tokio::sync::Mutex;

use crate::cluster::manager::ClusterManager;
use crate::cluster::traits::ClusterManagerTrait;
use crate::config::access_secrets::AccessSecret;
use crate::protocol::types::FileListState;
use crate::websocket::server::WS_CLOSE_HANDSHAKE_GRACE_SECONDS;

/// Shared application state, injected into all HTTP/WS handlers.
#[derive(Clone)]
pub struct AppState {
    /// `SeaORM` connection for HTTP handler database operations.
    /// Backed by `MySQL` in production, `SQLite` in tests.
    pub db: sea_orm::DatabaseConnection,
    /// Cluster lifecycle manager for WebSocket connections and file transfer sessions.
    pub cluster_manager: Arc<dyn ClusterManagerTrait>,
    /// In-flight file list requests keyed by cache UUID.
    pub file_list_map: Arc<DashMap<String, Arc<Mutex<FileListState>>>>,
    /// JWT signing secrets loaded from `access_secrets.json`.
    pub jwt_secrets: Arc<Vec<AccessSecret>>,
    /// Override for client timeout seconds. `None` uses the static default.
    pub client_timeout_seconds: Option<u64>,
}

/// Initialize all components and start HTTP + WebSocket servers.
///
/// # Errors
///
/// Returns an error if:
/// - Environment variables are missing or invalid
/// - Database connection fails
/// - Configuration files cannot be read or parsed
/// - HTTP or WebSocket server fails to start
pub async fn run() -> anyhow::Result<()> {
    tracing::info!("ADACS Job Controller starting...");

    tracing::debug!("Loading access secrets configuration");
    let access_secret_path =
        std::env::var(crate::config::settings::ACCESS_SECRET_CONFIG_FILE_ENV_VARIABLE)
            .unwrap_or_else(|_| "config/access_secrets.json".to_string());
    tracing::trace!("Access secrets path: {}", access_secret_path);

    let jwt_secrets = crate::config::access_secrets::load_access_secrets(std::path::Path::new(
        &access_secret_path,
    ))?;
    tracing::debug!("Loaded {} access secrets", jwt_secrets.len());

    tracing::debug!("Connecting to database");
    let db_url = format!(
        "mysql://{}:{}@{}:{}/{}?ssl-mode=disabled",
        &*crate::config::settings::DATABASE_USER,
        &*crate::config::settings::DATABASE_PASSWORD,
        &*crate::config::settings::DATABASE_HOST,
        &*crate::config::settings::DATABASE_PORT,
        &*crate::config::settings::DATABASE_SCHEMA,
    );
    tracing::trace!("Database URL constructed (credentials hidden)");
    let db = sea_orm::Database::connect(&db_url).await?;
    tracing::debug!("Database connection established, applying migrations");
    crate::db::migration::migrator::Migrator::up(&db, None).await?;
    tracing::info!("Database migrations applied successfully");

    tracing::debug!("Initializing file list cache map");
    let file_list_map: Arc<DashMap<_, _>> = Arc::new(DashMap::new());

    tracing::debug!("Loading cluster configuration");
    let cluster_config_path =
        std::env::var(crate::config::settings::CLUSTER_CONFIG_FILE_ENV_VARIABLE)
            .unwrap_or_else(|_| "config/clusters.json".to_string());
    tracing::trace!("Cluster config path: {}", cluster_config_path);
    let cluster_configs =
        crate::config::clusters::load_cluster_configs(std::path::Path::new(&cluster_config_path))?;
    tracing::debug!("Loaded {} cluster configurations", cluster_configs.len());

    tracing::debug!("Creating cluster manager");
    let cluster_manager =
        ClusterManager::new(cluster_configs, db.clone(), Arc::clone(&file_list_map));
    tracing::debug!("Starting cluster manager background tasks");
    cluster_manager.start_tasks();
    let cluster_manager: Arc<dyn ClusterManagerTrait> = cluster_manager;

    tracing::debug!("Building application state");
    let app_state = AppState {
        db,
        cluster_manager: Arc::clone(&cluster_manager),
        file_list_map,
        jwt_secrets: Arc::new(jwt_secrets),
        client_timeout_seconds: None,
    };

    tracing::debug!("Creating HTTP router with middleware");
    let http_router = crate::http::server::create_router(app_state.clone());

    tracing::debug!("Creating WebSocket router");
    let ws_router = axum::Router::new()
        .route(
            "/job/ws/",
            axum::routing::get(crate::websocket::server::ws_handler),
        )
        .with_state(app_state);

    let http_port = *crate::config::settings::HTTP_PORT;
    let ws_port = *crate::config::settings::WEBSOCKET_PORT;

    tracing::info!("Binding HTTP server to 0.0.0.0:{http_port}");
    tracing::info!("Binding WebSocket server to 0.0.0.0:{ws_port}");

    let http_listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{http_port}")).await?;
    tracing::debug!("HTTP listener bound successfully");
    let ws_listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{ws_port}")).await?;
    tracing::debug!("WebSocket listener bound successfully");

    tracing::info!("ADACS Job Controller fully initialized, accepting connections");

    let shutdown_manager: Arc<dyn ClusterManagerTrait> = Arc::clone(&cluster_manager);

    tokio::try_join!(
        async {
            tracing::debug!("HTTP server task started");
            axum::serve(http_listener, http_router)
                .with_graceful_shutdown(build_application_shutdown_future(Arc::clone(
                    &shutdown_manager,
                )))
                .await
                .map_err(anyhow::Error::from)
        },
        async {
            tracing::debug!("WebSocket server task started");
            axum::serve(
                ws_listener,
                ws_router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .with_graceful_shutdown(build_application_shutdown_future(Arc::clone(
                &shutdown_manager,
            )))
            .await
            .map_err(anyhow::Error::from)
        },
    )?;

    Ok(())
}

/// Compose the bounded application-shutdown future used by both
/// `axum::serve` `with_graceful_shutdown` arms.
///
/// The future:
/// 1. Waits for `SIGINT` or `SIGTERM` (Linux only; other platforms
///    fall back to waiting for `SIGINT` only).
/// 2. Calls [`ClusterManager::begin_application_shutdown`] which sets
///    the dedicated-admission flag and synchronously triggers every
///    registered file-download session with reason
///    [`crate::cluster::file_download::DownloadShutdownReason::ApplicationShutdown`].
/// 3. Sleeps `WS_CLOSE_HANDSHAKE_GRACE_SECONDS` so handlers and
///    forwarders can complete the existing five-second close handshake
///    within the same bound reused by the WebSocket handler.
/// 4. Awaits [`crate::cluster::cluster::Cluster::terminate_download_tasks`]
///    on every dedicated download cluster currently retained by the
///    manager. The awaitable reuses the same five-second bound and is
///    idempotent; sessions that finished naturally short-circuit.
/// 5. Resolves so axum proceeds with its own connection drain.
///
/// No `DashMap`, lifecycle, pause/resume, or connection lock is held
/// across any await in this future. No new task is spawned.
async fn build_application_shutdown_future(cluster_manager: Arc<dyn ClusterManagerTrait>) {
    wait_for_shutdown_signal().await;

    tracing::info!("Application shutdown: signalling dedicated download sessions");
    let triggered = cluster_manager.begin_application_shutdown();
    tracing::info!(
        "Application shutdown: triggered {} dedicated download session(s); draining for {}s",
        triggered,
        WS_CLOSE_HANDSHAKE_GRACE_SECONDS,
    );

    // Reuse the existing five-second close bound instead of inventing a
    // new long timeout. After this sleep the WS handlers will have
    // observed `ApplicationShutdown`, sent Close, and either completed
    // gracefully or fallen back through the existing forced-fallback
    // path.
    tokio::time::sleep(Duration::from_secs(WS_CLOSE_HANDSHAKE_GRACE_SECONDS)).await;

    // Drain every retained dedicated download cluster's scheduler,
    // prune, and resend task handles within the same bound. The await
    // is bounded by `WS_CLOSE_HANDSHAKE_GRACE_SECONDS` because
    // `Cluster::terminate_download_tasks` reuses that constant.
    let weak_clusters = cluster_manager.dedicated_download_clusters();
    for weak in weak_clusters {
        if let Some(cluster) = weak.upgrade() {
            cluster.terminate_download_tasks().await;
        }
    }

    tracing::debug!("Application shutdown: axum graceful shutdown may now proceed");
}

/// Block until `SIGINT` or `SIGTERM` is received.
///
/// On Linux both signals are honoured. On other targets only `SIGINT`
/// is bound because `tokio::signal::unix::SignalKind::terminate()` is
/// unavailable.
async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut sigint = signal(SignalKind::interrupt()).expect("install SIGINT handler");
        let mut sigterm = signal(SignalKind::terminate()).expect("install SIGTERM handler");
        tokio::select! {
            _ = sigint.recv() => tracing::info!("Received SIGINT, beginning bounded shutdown"),
            _ = sigterm.recv() => tracing::info!("Received SIGTERM, beginning bounded shutdown"),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!("Received Ctrl+C, beginning bounded shutdown");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_state_is_clone() {
        fn assert_clone<T: Clone>() {}
        assert_clone::<AppState>();
    }

    #[test]
    fn test_app_state_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<AppState>();
    }
}
