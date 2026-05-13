use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::Mutex;

use crate::cluster::manager::ClusterManager;
use crate::cluster::traits::ClusterManagerTrait;
use crate::config::access_secrets::AccessSecret;
use crate::protocol::types::FileListState;

/// Shared application state, injected into all HTTP/WS handlers.
#[derive(Clone)]
pub struct AppState {
    /// `SeaORM` connection for HTTP handler database operations.
    /// Backed by `MySQL` in production, `SQLite` in tests.
    pub db: sea_orm::DatabaseConnection,
    pub cluster_manager: Arc<dyn ClusterManagerTrait>,
    pub file_list_map: Arc<DashMap<String, Arc<Mutex<FileListState>>>>,
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

    let access_secret_path =
        std::env::var(crate::config::settings::ACCESS_SECRET_CONFIG_FILE_ENV_VARIABLE)
            .unwrap_or_else(|_| "config/access_secrets.json".to_string());

    let jwt_secrets = crate::config::access_secrets::load_access_secrets(std::path::Path::new(
        &access_secret_path,
    ))?;

    // SeaORM connection for all database operations
    let db_url = format!(
        "mysql://{}:{}@{}/{}?ssl-mode=disabled",
        &*crate::config::settings::DATABASE_USER,
        &*crate::config::settings::DATABASE_PASSWORD,
        &*crate::config::settings::DATABASE_HOST,
        &*crate::config::settings::DATABASE_SCHEMA,
    );
    let db = sea_orm::Database::connect(&db_url).await?;
    tracing::info!("SeaORM connection established");

    let file_list_map: Arc<DashMap<_, _>> = Arc::new(DashMap::new());

    let cluster_config_path =
        std::env::var(crate::config::settings::CLUSTER_CONFIG_FILE_ENV_VARIABLE)
            .unwrap_or_else(|_| "config/clusters.json".to_string());
    let cluster_configs =
        crate::config::clusters::load_cluster_configs(std::path::Path::new(&cluster_config_path))?;
    let cluster_manager =
        ClusterManager::new(cluster_configs, db.clone(), Arc::clone(&file_list_map));
    cluster_manager.start_tasks();
    let cluster_manager: Arc<dyn ClusterManagerTrait> = cluster_manager;

    let app_state = AppState {
        db,
        cluster_manager: Arc::clone(&cluster_manager),
        file_list_map,
        jwt_secrets: Arc::new(jwt_secrets),
        client_timeout_seconds: None,
    };

    let http_router = crate::http::server::create_router(app_state.clone());

    let ws_router = axum::Router::new()
        .route(
            "/job/ws/",
            axum::routing::get(crate::websocket::server::ws_handler),
        )
        .with_state(app_state);

    let http_port = *crate::config::settings::HTTP_PORT;
    let ws_port = *crate::config::settings::WEBSOCKET_PORT;

    tracing::info!("Starting HTTP server on 0.0.0.0:{http_port}");
    tracing::info!("Starting WebSocket server on 0.0.0.0:{ws_port}");

    let http_listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{http_port}")).await?;
    let ws_listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{ws_port}")).await?;

    tokio::try_join!(
        async {
            axum::serve(http_listener, http_router)
                .await
                .map_err(anyhow::Error::from)
        },
        async {
            axum::serve(ws_listener, ws_router)
                .await
                .map_err(anyhow::Error::from)
        },
    )?;

    Ok(())
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
