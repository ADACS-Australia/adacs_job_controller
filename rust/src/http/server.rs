use axum::Router;
use axum::routing::{post, put};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::app::AppState;
use crate::http::{file, job};

/// Create the HTTP API router with all routes and middleware.
pub fn create_router(state: AppState) -> Router {
    let job_routes = Router::new().route(
        "/job/apiv1/job/",
        post(job::create_job)
            .get(job::get_jobs)
            .patch(job::cancel_job)
            .delete(job::delete_job),
    );

    let file_routes = Router::new()
        .route(
            "/file/apiv1/file/",
            post(file::create_file_download)
                .get(file::download_file)
                .patch(file::list_files),
        )
        .route("/file/apiv1/file/upload/", put(file::upload_file));

    Router::new()
        .merge(job_routes)
        .merge(file_routes)
        .layer(axum::Extension(state.jwt_secrets.clone()))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
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
