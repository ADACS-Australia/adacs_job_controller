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

/// Compute the rate-limiter replenish interval in milliseconds when rate limiting
/// is enabled and the governor parameters are valid.
///
/// Returns `None` when rate limiting is disabled (`requests_per_second == 0`) or
/// when the governor parameters are invalid (e.g. a zero burst size), so callers
/// can fall back to serving without rate limiting instead of panicking.
fn rate_limiter_interval_ms(requests_per_second: u64, burst_size: u32) -> Option<u64> {
    if requests_per_second == 0 || burst_size == 0 {
        return None;
    }
    Some((1000u64.saturating_div(requests_per_second)).max(1))
}

/// Create the HTTP API router with all routes and middleware.
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
    let requests_per_second = *RATE_LIMIT_REQUESTS_PER_SECOND;
    let burst_size = *RATE_LIMIT_BURST_SIZE;
    if requests_per_second > 0 {
        tracing::debug!(
            "HTTP: Enabling rate limiting ({} req/s, burst {})",
            requests_per_second,
            burst_size
        );
    }
    if let Some(interval_ms) = rate_limiter_interval_ms(requests_per_second, burst_size) {
        let mut config_builder = GovernorConfigBuilder::default();
        config_builder.per_millisecond(interval_ms);
        config_builder.burst_size(burst_size);
        if let Some(governor_config) = config_builder.finish() {
            router = router.layer(GovernorLayer::new(Arc::new(governor_config)));
            tracing::trace!(
                "HTTP: Rate limiter configured with {}ms interval",
                interval_ms
            );
        } else {
            tracing::warn!(
                "HTTP: Invalid rate limiter config (burst_size {}, interval {}ms); rate limiting disabled",
                burst_size,
                interval_ms
            );
        }
    } else if requests_per_second > 0 {
        tracing::warn!(
            "HTTP: Invalid rate limiter config (burst_size {}, interval {}ms); rate limiting disabled",
            burst_size,
            (1000u64.saturating_div(requests_per_second)).max(1)
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

    #[test]
    fn rate_limiter_interval_ms_is_none_when_rate_limiting_disabled() {
        // A zero requests-per-second value disables rate limiting entirely.
        assert_eq!(rate_limiter_interval_ms(0, 50), None);
    }

    #[test]
    fn rate_limiter_interval_ms_is_none_for_zero_burst_size() {
        // A zero burst size is invalid for governor; the router must fall back
        // to serving without rate limiting instead of panicking.
        assert_eq!(rate_limiter_interval_ms(10, 0), None);
    }

    #[test]
    fn rate_limiter_interval_ms_computes_interval_from_requests_per_second() {
        // 10 req/s replenishes one element every 100ms.
        assert_eq!(rate_limiter_interval_ms(10, 50), Some(100));
        // 500 req/s replenishes one element every 2ms.
        assert_eq!(rate_limiter_interval_ms(500, 50), Some(2));
    }

    #[test]
    fn rate_limiter_interval_ms_floors_interval_at_one_millisecond() {
        // Very high request rates cannot go below a 1ms replenish interval.
        assert_eq!(rate_limiter_interval_ms(1000, 50), Some(1));
        assert_eq!(rate_limiter_interval_ms(1_000_000, 50), Some(1));
    }
}
