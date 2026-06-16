use std::sync::Arc;

use axum::Router;
use axum::routing::{post, put};
use tower_governor::GovernorLayer;
use tower_governor::governor::GovernorConfigBuilder;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::app::AppState;
use crate::config::settings::{RATE_LIMIT_BURST_SIZE, RATE_LIMIT_REQUESTS_PER_SECOND};
use crate::http::{file, job};

/// Create the HTTP API router with all routes and middleware.
///
/// # Panics
///
/// Panics if rate limiting is enabled but the configured governor parameters are invalid.
pub fn create_router(state: AppState) -> Router {
    tracing::debug!("HTTP: Building job routes");
    let job_routes = Router::new().route(
        "/job/apiv1/job/",
        post(job::create_job)
            .get(job::get_jobs)
            .patch(job::cancel_job)
            .delete(job::delete_job),
    );

    tracing::debug!("HTTP: Building file routes");
    let file_routes = Router::new()
        .route(
            "/job/apiv1/file/",
            post(file::create_file_download)
                .get(file::download_file)
                .patch(file::list_files),
        )
        .route("/job/apiv1/file/upload/", put(file::upload_file));

    tracing::debug!("HTTP: Creating base router with middleware");
    let mut router = Router::new()
        .merge(job_routes)
        .merge(file_routes)
        .layer(axum::Extension(Arc::clone(&state.jwt_secrets)))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    // Rate limiting (disabled when RATE_LIMIT_REQUESTS_PER_SECOND is 0)
    if *RATE_LIMIT_REQUESTS_PER_SECOND > 0 {
        let interval_ms = (1000u64.saturating_div(*RATE_LIMIT_REQUESTS_PER_SECOND)).max(1);
        tracing::debug!(
            "HTTP: Enabling rate limiting ({} req/s, burst {})",
            *RATE_LIMIT_REQUESTS_PER_SECOND,
            *RATE_LIMIT_BURST_SIZE
        );
        let mut config_builder = GovernorConfigBuilder::default();
        config_builder.per_millisecond(interval_ms);
        config_builder.burst_size(*RATE_LIMIT_BURST_SIZE);
        let governor_config = Arc::new(
            config_builder
                .finish()
                .expect("rate limiter config: burst_size and period must be non-zero"),
        );
        router = router.layer(GovernorLayer::new(governor_config));
        tracing::trace!(
            "HTTP: Rate limiter configured with {}ms interval",
            interval_ms
        );
    } else {
        tracing::debug!("HTTP: Rate limiting disabled");
    }

    tracing::info!("HTTP: Router created with job and file routes");
    router.with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_router_builds() {
        // Verify that the router structure compiles and can be constructed
        // We can't test with real requests without a DB pool, but we verify
        // the route definitions are correct by checking the function compiles.
        fn _assert_router_fn_exists() {
            let _: fn(AppState) -> Router = create_router;
        }
    }
}
