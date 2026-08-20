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
/// 3. Computes one absolute deadline of `WS_CLOSE_HANDSHAKE_GRACE_SECONDS`
///    that bounds the entire download shutdown operation.
/// 4. Drains every retained dedicated download cluster's
///    [`crate::cluster::cluster::Cluster::terminate_download_tasks`]
///    concurrently within that single deadline, while the WS close-handshake
///    window runs in parallel. The whole operation is globally bounded by
///    the deadline regardless of cluster or retained-task count.
/// 5. Resolves so axum proceeds with its own connection drain.
///
/// No `DashMap`, lifecycle, pause/resume, or connection lock is held
/// across any await in this future. No new task is spawned.
async fn build_application_shutdown_future(cluster_manager: Arc<dyn ClusterManagerTrait>) {
    wait_for_shutdown_signal().await;
    drain_application_shutdown(cluster_manager).await;
}

/// Drain all dedicated download sessions and retained cluster tasks within
/// one absolute deadline of `WS_CLOSE_HANDSHAKE_GRACE_SECONDS`.
///
/// The whole operation is globally bounded by the deadline regardless of the
/// number of clusters or retained task handles: sessions are triggered, all
/// retained cluster tasks are drained concurrently, and the WS close-handshake
/// window runs in parallel within the same grace period.
async fn drain_application_shutdown(cluster_manager: Arc<dyn ClusterManagerTrait>) {
    tracing::info!("Application shutdown: signalling dedicated download sessions");
    let triggered = cluster_manager.begin_application_shutdown();
    tracing::info!(
        "Application shutdown: triggered {} dedicated download session(s); draining for {}s",
        triggered,
        WS_CLOSE_HANDSHAKE_GRACE_SECONDS,
    );

    // One absolute deadline bounds the entire download shutdown operation:
    // the WS close-handshake window and the retained-task drain both run
    // within this single grace period.
    let deadline =
        tokio::time::Instant::now() + Duration::from_secs(WS_CLOSE_HANDSHAKE_GRACE_SECONDS);

    // Drain every retained dedicated download cluster's scheduler, prune,
    // and resend task handles concurrently. `terminate_download_tasks`
    // aborts all of a cluster's handles before awaiting any of them.
    let drains = cluster_manager
        .dedicated_download_clusters()
        .into_iter()
        .filter_map(|weak| weak.upgrade())
        .map(|cluster| async move {
            cluster.terminate_download_tasks().await;
        });

    let drain = futures_util::future::join_all(drains);

    // Run the task drain concurrently with the close-handshake window so
    // WS handlers have the full grace period to complete, while the whole
    // operation stays within the single deadline.
    tokio::pin!(drain);
    tokio::select! {
        _ = &mut drain => {
            // Drain finished early; wait for the remaining close-handshake
            // window so WS handlers can complete gracefully.
            tokio::time::sleep_until(deadline).await;
        }
        () = tokio::time::sleep_until(deadline) => {}
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

    use crate::cluster::manager::ClusterManager;
    use crate::config::clusters::ClusterConfig;

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

    /// Verifies that application shutdown drains all dedicated download
    /// clusters within one absolute grace-period deadline, regardless of the
    /// number of clusters or retained non-cooperative task handles.
    ///
    /// Uses Tokio's paused clock: virtual time is advanced by exactly
    /// `WS_CLOSE_HANDSHAKE_GRACE_SECONDS` and the drain must resolve, with
    /// every retained handle aborted and drained, without scaling with the
    /// cluster or handle count.
    #[tokio::test]
    async fn drain_application_shutdown_is_globally_bounded() {
        let db = sea_orm::Database::connect("sqlite::memory:")
            .await
            .expect("sqlite in-memory connection failed");
        crate::db::schema::create_test_schema(&db).await;

        let file_list_map = Arc::new(DashMap::new());
        let manager: Arc<ClusterManager> = ClusterManager::new(
            vec![ClusterConfig {
                name: "shutdown_cluster".to_string(),
                host: "127.0.0.1".to_string(),
                username: "test".to_string(),
                path: "/tmp".to_string(),
                key: String::new(),
                connection_type: "manual".to_string(),
                keytab: String::new(),
                kerberos_principal: String::new(),
                ltk: None,
            }],
            db,
            file_list_map,
        );

        let master = manager
            .get_cluster_by_name("shutdown_cluster")
            .expect("manager should expose the configured cluster");

        // Create several dedicated download clusters and inject a
        // non-cooperative task handle into each so the drain cannot rely on
        // graceful `running`-flag observation.
        for i in 0..5 {
            manager
                .create_file_download(&master, &format!("dl-{i}"))
                .await;
        }
        let concrete = manager.dedicated_download_clusters_concrete();
        assert_eq!(concrete.len(), 5);
        for cluster in &concrete {
            cluster.push_non_cooperative_task_handle();
        }

        // Pause the clock only after DB and cluster setup so the connection
        // pool is established in real time.
        tokio::time::pause();

        let trait_manager: Arc<dyn ClusterManagerTrait> = manager.clone();
        let drain = tokio::spawn(drain_application_shutdown(trait_manager));

        // Let the drain task start and register its deadline.
        tokio::task::yield_now().await;

        // Advance virtual time slightly past the grace period so the deadline
        // timer fires (Tokio's timer wheel requires the clock to pass the
        // deadline, not merely reach it).
        let grace = Duration::from_secs(WS_CLOSE_HANDSHAKE_GRACE_SECONDS);
        tokio::time::advance(grace + Duration::from_millis(1)).await;
        tokio::task::yield_now().await;

        assert!(
            drain.is_finished(),
            "application shutdown drain must resolve within the single grace-period deadline"
        );
        drain.await.expect("drain task should not panic");

        // Every retained handle must have been aborted and drained.
        for cluster in manager.dedicated_download_clusters_concrete() {
            assert_eq!(
                cluster.retained_download_task_count(),
                0,
                "no retained task handles after bounded drain"
            );
        }
    }
}
