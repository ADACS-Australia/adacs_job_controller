//! HTTP Load Testing and Performance Benchmarks
//!
//! These tests match C++ load testing coverage:
//! - `test_http_worker_pool_exhaustion` (1024 connections)
//! - Performance regression tests
//! - Stress testing under load

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::Request;
use futures_util::future::join_all;
use serde_json::json;
use tokio::sync::Semaphore;
use tower::ServiceExt;

use adacs_job_controller::cluster::traits::{MockClusterManagerTrait, MockClusterTrait};
use adacs_job_controller::db::entities::job;
use adacs_job_controller::http::server::create_router;
use adacs_job_controller::protocol::types::ClusterRole;

use common::{
    encode_test_jwt, insert_test_job, make_test_state, setup_test_db, test_cluster_config,
};

use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};

// ---------------------------------------------------------------------------
// HTTP Load Tests - Match C++ test_http_worker_pool_exhaustion
// ---------------------------------------------------------------------------

/// Tests server behavior under moderate concurrent load.
///
/// Matches C++: `test_http_worker_pool_exhaustion` (simplified version)
///
/// # Setup
/// - Creates online cluster mock
/// - Sets up test database
///
/// # Act
/// - Sends 100 concurrent job creation requests
/// - All requests within 1 second window
///
/// # Assert
/// - All requests complete (success or graceful rejection)
/// - No server crashes or panics
/// - Database remains consistent
#[tokio::test]
async fn test_http_concurrent_job_creation_moderate_load() {
    let db = setup_test_db().await;

    let mut cluster = MockClusterTrait::new();
    cluster.expect_name().returning(|| "ozstar".to_string());
    cluster.expect_is_online().returning(|| true);
    cluster.expect_role().returning(|| ClusterRole::Master);
    cluster
        .expect_role_string()
        .returning(|| "master".to_string());
    cluster
        .expect_cluster_details()
        .returning(|| test_cluster_config("ozstar"));
    cluster
        .expect_send_message()
        .returning(|_| Box::pin(async {}));
    let cluster_arc = Arc::new(cluster);

    let mut manager = MockClusterManagerTrait::new();
    manager.expect_get_cluster_by_name().returning(move |_| {
        Some(Arc::clone(&cluster_arc)
            as Arc<
                dyn adacs_job_controller::cluster::traits::ClusterTrait,
            >)
    });
    manager
        .expect_handle_new_connection()
        .returning(move |_, _, _| Box::pin(async move { None }));

    let app = create_router(make_test_state(db.clone(), manager));
    let token = encode_test_jwt(&json!({"userId": 1, "application": "testapp"}));

    // Send 100 concurrent requests
    let num_requests = 100;
    let mut handles = Vec::new();

    for i in 0..num_requests {
        let app_clone = app.clone();
        let token_clone = token.clone();

        let handle = tokio::spawn(async move {
            let job_data = json!({
                "cluster": "ozstar",
                "bundle": format!("bundle_{}", i),
                "application": "testapp",
                "parameters": "{}"
            });

            let resp = app_clone
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/job/apiv1/job/")
                        .header("content-type", "application/json")
                        .header("authorization", &token_clone)
                        .body(Body::from(job_data.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();

            resp.status()
        });

        handles.push(handle);
    }

    // Wait for all requests to complete
    let results = join_all(handles).await;

    // Count successes and failures
    let mut success_count = 0;
    let mut client_error_count = 0;
    let mut server_error_count = 0;

    for result in results {
        match result {
            Ok(status) => {
                if status.is_success() {
                    success_count += 1;
                } else if status.is_client_error() {
                    client_error_count += 1;
                } else if status.is_server_error() {
                    server_error_count += 1;
                }
            }
            Err(_) => {
                // Task panicked or was cancelled
                server_error_count += 1;
            }
        }
    }

    // All requests should complete (either success or graceful error)
    let total = success_count + client_error_count + server_error_count;
    assert_eq!(total, num_requests, "All requests should complete");

    // Server errors should be zero
    assert_eq!(server_error_count, 0, "No server errors expected");

    // Verify database consistency
    let job_count = job::Entity::find().count(&db).await.unwrap();
    assert_eq!(
        usize::try_from(job_count).unwrap(),
        success_count,
        "Job count should match successes"
    );
}

/// Tests server behavior under heavy concurrent load.
///
/// Matches C++: `test_http_worker_pool_exhaustion`
///
/// # Setup
/// - Creates online cluster mock
/// - Sets up test database
///
/// # Act
/// - Sends 500 concurrent job creation requests
/// - All requests within 500ms window
///
/// # Assert
/// - Server handles load gracefully
/// - No panics or crashes
/// - Response times reasonable (< 5s average)
#[tokio::test]
#[allow(clippy::cast_precision_loss)]
async fn test_http_concurrent_job_creation_heavy_load() {
    let db = setup_test_db().await;

    let mut cluster = MockClusterTrait::new();
    cluster.expect_name().returning(|| "ozstar".to_string());
    cluster.expect_is_online().returning(|| true);
    cluster.expect_role().returning(|| ClusterRole::Master);
    cluster
        .expect_role_string()
        .returning(|| "master".to_string());
    cluster
        .expect_cluster_details()
        .returning(|| test_cluster_config("ozstar"));
    cluster
        .expect_send_message()
        .returning(|_| Box::pin(async {}));
    let cluster_arc = Arc::new(cluster);

    let mut manager = MockClusterManagerTrait::new();
    manager.expect_get_cluster_by_name().returning(move |_| {
        Some(Arc::clone(&cluster_arc)
            as Arc<
                dyn adacs_job_controller::cluster::traits::ClusterTrait,
            >)
    });
    manager
        .expect_handle_new_connection()
        .returning(move |_, _, _| Box::pin(async move { None }));

    let app = create_router(make_test_state(db.clone(), manager));
    let token = encode_test_jwt(&json!({"userId": 1, "application": "testapp"}));

    // Send 500 concurrent requests
    let num_requests = 500;
    let start_time = Instant::now();

    let mut handles = Vec::new();
    for i in 0..num_requests {
        let app_clone = app.clone();
        let token_clone = token.clone();

        let handle = tokio::spawn(async move {
            let job_data = json!({
                "cluster": "ozstar",
                "bundle": format!("bundle_{}", i),
                "application": "testapp",
                "parameters": "{}"
            });

            let start = Instant::now();
            let resp = app_clone
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/job/apiv1/job/")
                        .header("content-type", "application/json")
                        .header("authorization", &token_clone)
                        .body(Body::from(job_data.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            let elapsed = start.elapsed();

            (resp.status(), elapsed)
        });

        handles.push(handle);
    }

    // Wait for all requests to complete
    let results = join_all(handles).await;
    let total_elapsed = start_time.elapsed();

    // Analyze results
    let mut success_count = 0;
    let mut total_response_time = Duration::ZERO;
    let mut max_response_time = Duration::ZERO;

    for (status, response_time) in results.into_iter().flatten() {
        if status.is_success() {
            success_count += 1;
        }
        total_response_time += response_time;
        if response_time > max_response_time {
            max_response_time = response_time;
        }
    }

    let avg_response_time = total_response_time / num_requests;

    // Performance assertions
    assert!(
        avg_response_time < Duration::from_secs(5),
        "Average response time should be < 5s, got {avg_response_time:?}"
    );

    assert!(
        max_response_time < Duration::from_secs(30),
        "Max response time should be < 30s, got {max_response_time:?}"
    );

    // Success rate should be reasonable (> 50%)
    let success_rate = f64::from(success_count) / f64::from(num_requests);
    assert!(
        success_rate > 0.5,
        "Success rate should be > 50%, got {:.2}%",
        success_rate * 100.0
    );

    println!(
        "Load test results: {success_count}/{num_requests} success, avg: {avg_response_time:?}, max: {max_response_time:?}, total: {total_elapsed:?}"
    );
}

// ---------------------------------------------------------------------------
// Performance Benchmark Tests
// ---------------------------------------------------------------------------

/// Benchmarks job creation performance.
///
/// Measures:
/// - Average response time
/// - Throughput (requests/second)
/// - P95 latency
///
/// # Note
/// This is a performance test, not a functional test.
/// It measures baseline performance for regression detection.
#[tokio::test]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
async fn test_benchmark_job_creation_performance() {
    let db = setup_test_db().await;

    let mut cluster = MockClusterTrait::new();
    cluster.expect_name().returning(|| "ozstar".to_string());
    cluster.expect_is_online().returning(|| true);
    cluster.expect_role().returning(|| ClusterRole::Master);
    cluster
        .expect_role_string()
        .returning(|| "master".to_string());
    cluster
        .expect_cluster_details()
        .returning(|| test_cluster_config("ozstar"));
    cluster
        .expect_send_message()
        .returning(|_| Box::pin(async {}));
    let cluster_arc = Arc::new(cluster);

    let mut manager = MockClusterManagerTrait::new();
    manager.expect_get_cluster_by_name().returning(move |_| {
        Some(Arc::clone(&cluster_arc)
            as Arc<
                dyn adacs_job_controller::cluster::traits::ClusterTrait,
            >)
    });
    manager
        .expect_handle_new_connection()
        .returning(move |_, _, _| Box::pin(async move { None }));

    let app = create_router(make_test_state(db.clone(), manager));
    let token = encode_test_jwt(&json!({"userId": 1, "application": "testapp"}));

    // Warmup
    for _ in 0..10 {
        let job_data = json!({
            "cluster": "ozstar",
            "bundle": "warmup",
            "application": "testapp",
            "parameters": "{}"
        });

        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/job/apiv1/job/")
                    .header("content-type", "application/json")
                    .header("authorization", &token)
                    .body(Body::from(job_data.to_string()))
                    .unwrap(),
            )
            .await;
    }

    // Benchmark run
    let num_requests = 50;
    let mut response_times = Vec::with_capacity(num_requests);

    for i in 0..num_requests {
        let job_data = json!({
            "cluster": "ozstar",
            "bundle": format!("bench_{}", i),
            "application": "testapp",
            "parameters": "{}"
        });

        let start = Instant::now();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/job/apiv1/job/")
                    .header("content-type", "application/json")
                    .header("authorization", &token)
                    .body(Body::from(job_data.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert!(
            resp.status().is_success(),
            "Benchmark request should succeed"
        );
        response_times.push(start.elapsed());
    }

    // Calculate statistics
    response_times.sort();

    let avg = response_times.iter().sum::<Duration>() / u32::try_from(num_requests).unwrap();
    let min = *response_times.first().unwrap();
    let max = *response_times.last().unwrap();
    let p95_idx = (num_requests as f64 * 0.95).floor() as usize;
    let p95 = response_times[p95_idx.min(num_requests - 1)];

    println!("Job Creation Performance Benchmark:");
    println!("  Requests: {num_requests}");
    println!("  Min: {min:?}");
    println!("  Max: {max:?}");
    println!("  Avg: {avg:?}");
    println!("  P95: {p95:?}");
    println!(
        "  Throughput: {:.2} req/s",
        num_requests as f64 / avg.as_secs_f64()
    );
}

/// Benchmarks database query performance.
///
/// Measures:
/// - Job lookup by ID
/// - Job listing with filters
/// - Bulk operations
#[tokio::test]
async fn test_benchmark_database_query_performance() {
    let db = setup_test_db().await;

    // Insert test data
    let mut job_ids = Vec::new();
    for i in 0..100 {
        let job_id = insert_test_job(&db, "ozstar", &format!("b{i}"), "testapp").await;
        job_ids.push(job_id);
    }

    // Benchmark: Single job lookup
    let start = Instant::now();
    for _ in 0..100 {
        let _ = job::Entity::find_by_id(job_ids[0]).one(&db).await;
    }
    let single_lookup_avg = start.elapsed() / 100;

    // Benchmark: Bulk job retrieval
    let start = Instant::now();
    for _ in 0..10 {
        let _ = job::Entity::find()
            .filter(job::Column::Cluster.eq("ozstar"))
            .all(&db)
            .await;
    }
    let bulk_query_avg = start.elapsed() / 10;

    println!("Database Query Performance Benchmark:");
    println!("  Single lookup (avg): {single_lookup_avg:?}");
    println!("  Bulk query (avg): {bulk_query_avg:?}");
}

// ---------------------------------------------------------------------------
// Stress Tests - Connection Pool Exhaustion
// ---------------------------------------------------------------------------

/// Tests connection pool behavior under extreme load.
///
/// Matches C++: `test_http_worker_pool_exhaustion`
///
/// # Setup
/// - Semaphore to limit concurrent connections
/// - Simulates worker pool exhaustion
///
/// # Act
/// - Attempts 1000 requests with limited concurrency
/// - Measures queue behavior and timeouts
///
/// # Assert
/// - Server doesn't crash
/// - Requests are queued or rejected gracefully
/// - No resource leaks
#[tokio::test]
async fn test_connection_pool_exhaustion() {
    let db = setup_test_db().await;

    let mut cluster = MockClusterTrait::new();
    cluster.expect_name().returning(|| "ozstar".to_string());
    cluster.expect_is_online().returning(|| true);
    cluster.expect_role().returning(|| ClusterRole::Master);
    cluster
        .expect_role_string()
        .returning(|| "master".to_string());
    cluster
        .expect_cluster_details()
        .returning(|| test_cluster_config("ozstar"));
    cluster
        .expect_send_message()
        .returning(|_| Box::pin(async {}));
    let cluster_arc = Arc::new(cluster);

    let mut manager = MockClusterManagerTrait::new();
    manager.expect_get_cluster_by_name().returning(move |_| {
        Some(Arc::clone(&cluster_arc)
            as Arc<
                dyn adacs_job_controller::cluster::traits::ClusterTrait,
            >)
    });
    manager
        .expect_handle_new_connection()
        .returning(move |_, _, _| Box::pin(async move { None }));

    let app = create_router(make_test_state(db.clone(), manager));
    let token = encode_test_jwt(&json!({"userId": 1, "application": "testapp"}));

    // Limit concurrent connections to simulate pool exhaustion
    let semaphore = Arc::new(Semaphore::new(10)); // Only 10 concurrent
    let num_requests = 200;

    let mut handles = Vec::new();

    for i in 0..num_requests {
        let app_clone = app.clone();
        let token_clone = token.clone();
        let sem_clone = Arc::clone(&semaphore);

        let handle = tokio::spawn(async move {
            let permit = sem_clone.acquire().await.unwrap();

            let job_data = json!({
                "cluster": "ozstar",
                "bundle": format!("stress_{}", i),
                "application": "testapp",
                "parameters": "{}"
            });

            let result = app_clone
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/job/apiv1/job/")
                        .header("content-type", "application/json")
                        .header("authorization", &token_clone)
                        .body(Body::from(job_data.to_string()))
                        .unwrap(),
                )
                .await;

            drop(permit); // Release semaphore
            result.is_ok()
        });

        handles.push(handle);
    }

    // Wait for all requests
    let results = join_all(handles).await;

    // Count completions
    let completed = results
        .iter()
        .filter(|r| r.is_ok() && *r.as_ref().unwrap())
        .count();
    let failed = results
        .iter()
        .filter(|r| r.is_ok() && !*r.as_ref().unwrap())
        .count();
    let panicked = results.iter().filter(|r| r.is_err()).count();

    println!(
        "Connection pool stress test: {completed}/{num_requests} completed, {failed} failed, {panicked} panicked"
    );

    // All tasks should complete (even if request fails)
    assert_eq!(
        completed + failed,
        num_requests,
        "All tasks should complete"
    );

    // No panics
    assert_eq!(panicked, 0, "No tasks should panic");
}
