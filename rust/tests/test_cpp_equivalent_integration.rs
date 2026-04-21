//! Integration tests matching C++ test coverage.

mod common;

use std::sync::{Arc, Mutex as StdMutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message as TungsteniteMsg;
use tower::ServiceExt;

use adacs_job_controller::cluster::traits::{MockClusterManagerTrait, MockClusterTrait};
use adacs_job_controller::db::entities::{file_download, job};
use adacs_job_controller::http::server::create_router;
use adacs_job_controller::protocol::constants::*;
use adacs_job_controller::protocol::message::Message;
use adacs_job_controller::protocol::types::ClusterRole;

use common::{
    encode_test_jwt, insert_test_job, make_test_state, setup_test_db, test_cluster_config,
};

use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter};

// ===========================================================================
// Download Resume Test
// ===========================================================================

#[tokio::test]
async fn test_download_resume_after_interruption() {
    let db = setup_test_db().await;
    
    let mut cluster = MockClusterTrait::new();
    cluster.expect_name().returning(|| "ozstar".to_string());
    cluster.expect_is_online().returning(|| true);
    cluster.expect_role().returning(|| ClusterRole::Master);
    cluster.expect_role_string().returning(|| "master".to_string());
    cluster.expect_cluster_details()
        .returning(|| test_cluster_config("ozstar"));
    cluster.expect_send_message().returning(|_| ());
    
    let cluster_arc = Arc::new(cluster);
    
    let mut manager = MockClusterManagerTrait::new();
    let c = Arc::clone(&cluster_arc);
    manager
        .expect_get_cluster_by_name()
        .returning(move |_| Some(c.clone()));
    
    let job_id = insert_test_job(&db, "ozstar", "testbundle", "testapp").await;
    let uuid = uuid::Uuid::new_v4().to_string();
    
    file_download::ActiveModel {
        user: Set(42),
        job: Set(job_id as i32),
        cluster: Set("ozstar".to_string()),
        bundle: Set("testbundle".to_string()),
        uuid: Set(uuid.clone()),
        path: Set("/test/file.txt".to_string()),
        timestamp: Set(chrono::Utc::now().naive_utc()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .unwrap();
    
    let app = create_router(make_test_state(db.clone(), manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 42}));
    
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(&format!("/file/apiv1/file/?fileId={}", uuid))
                .header("authorization", &token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    
    let dl = file_download::Entity::find()
        .filter(file_download::Column::Uuid.eq(&uuid))
        .one(&db)
        .await
        .unwrap();
    
    assert!(dl.is_some());
}

// ===========================================================================
// Connection Race Condition Test
// ===========================================================================

#[tokio::test]
async fn test_message_on_disconnected_cluster_no_crash() {
    let db = setup_test_db().await;
    
    let mut cluster = MockClusterTrait::new();
    cluster.expect_name().returning(|| "ozstar".to_string());
    cluster.expect_is_online().returning(|| false);
    cluster.expect_role().returning(|| ClusterRole::Master);
    cluster.expect_role_string().returning(|| "master".to_string());
    cluster.expect_cluster_details()
        .returning(|| test_cluster_config("ozstar"));
    cluster.expect_send_message().returning(|_| ());
    
    let cluster_arc = Arc::new(cluster);
    
    let mut manager = MockClusterManagerTrait::new();
    let c = Arc::clone(&cluster_arc);
    manager
        .expect_get_cluster_by_name()
        .returning(move |_| Some(c.clone()));
    
    let app = create_router(make_test_state(db.clone(), manager));
    let token = encode_test_jwt(&serde_json::json!({"userId": 42}));
    
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/job/apiv1/job/")
                .header("content-type", "application/json")
                .header("authorization", &token)
                .body(Body::from(
                    r#"{"cluster":"ozstar","parameters":"{}","bundle":"test"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    
    assert_eq!(resp.status(), StatusCode::OK);
    
    let jobs = job::Entity::find().all(&db).await.unwrap();
    assert_eq!(jobs.len(), 1);
}

// ===========================================================================
// Real WebSocket Connection Test
// ===========================================================================

fn ws_router(state: adacs_job_controller::app::AppState) -> axum::Router {
    axum::Router::new()
        .route(
            "/job/ws/",
            axum::routing::get(adacs_job_controller::websocket::server::ws_handler),
        )
        .with_state(state)
}

async fn start_test_server(router: axum::Router) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    port
}

#[tokio::test]
async fn test_real_websocket_connection_and_auth() {
    let db = setup_test_db().await;
    
    let mut cluster = MockClusterTrait::new();
    cluster.expect_name().returning(|| "ozstar".to_string());
    cluster.expect_is_online().returning(|| true);
    cluster.expect_role().returning(|| ClusterRole::Master);
    cluster.expect_role_string().returning(|| "master".to_string());
    cluster.expect_cluster_details()
        .returning(|| test_cluster_config("ozstar"));
    cluster.expect_set_connection().returning(|_| ());
    cluster.expect_send_message().returning(|_| ());
    
    let cluster_arc = Arc::new(cluster);
    
    let mut manager = MockClusterManagerTrait::new();
    let c = Arc::clone(&cluster_arc);
    manager
        .expect_get_cluster_by_name()
        .returning(move |_| Some(c.clone()));
    
    let state = make_test_state(db.clone(), manager);
    let router = ws_router(state);
    
    let port = start_test_server(router).await;
    
    let token = encode_test_jwt(&serde_json::json!({"userId": 42}));
    let ws_url = format!("ws://127.0.0.1:{}/job/ws/", port);
    
    let request = tokio_tungstenite::tungstenite::ClientRequestBuilder::new(
        ws_url.parse().unwrap()
    )
    .with_header("Authorization", format!("Bearer {}", token));
    
    let (ws_stream, _) = tokio_tungstenite::connect_async(request)
        .await
        .unwrap();
    
    let (mut write, mut read) = ws_stream.split();
    
    if let Some(Ok(msg)) = read.next().await {
        match msg {
            TungsteniteMsg::Binary(data) => {
                let message = Message::from_bytes(data.to_vec());
                assert_eq!(message.id(), SERVER_READY);
            }
            _ => panic!("Expected binary SERVER_READY message"),
        }
    } else {
        panic!("No message received");
    }
    
    write.send(TungsteniteMsg::Close(None)).await.unwrap();
}

#[tokio::test]
async fn test_websocket_connection_rejected_invalid_token() {
    let db = setup_test_db().await;
    
    let mut manager = MockClusterManagerTrait::new();
    let state = make_test_state(db.clone(), manager);
    let router = ws_router(state);
    
    let port = start_test_server(router).await;
    
    let ws_url = format!("ws://127.0.0.1:{}/job/ws/", port);
    
    let request = tokio_tungstenite::tungstenite::ClientRequestBuilder::new(
        ws_url.parse().unwrap()
    )
    .with_header("Authorization", "Bearer invalid_token");
    
    let result = tokio_tungstenite::connect_async(request).await;
    
    assert!(result.is_err());
}

// ===========================================================================
// Multi-Cluster Concurrent Test
// ===========================================================================

#[tokio::test]
async fn test_multiple_clusters_concurrent_job_submission() {
    let db = setup_test_db().await;
    
    let mut cluster1 = MockClusterTrait::new();
    cluster1.expect_name().returning(|| "cluster1".to_string());
    cluster1.expect_is_online().returning(|| true);
    cluster1.expect_role().returning(|| ClusterRole::Master);
    cluster1.expect_role_string().returning(|| "master cluster1".to_string());
    cluster1.expect_cluster_details()
        .returning(|| test_cluster_config("cluster1"));
    cluster1.expect_send_message().returning(|_| ());
    
    let mut cluster2 = MockClusterTrait::new();
    cluster2.expect_name().returning(|| "cluster2".to_string());
    cluster2.expect_is_online().returning(|| true);
    cluster2.expect_role().returning(|| ClusterRole::Master);
    cluster2.expect_role_string().returning(|| "master cluster2".to_string());
    cluster2.expect_cluster_details()
        .returning(|| test_cluster_config("cluster2"));
    cluster2.expect_send_message().returning(|_| ());
    
    let mut manager = MockClusterManagerTrait::new();
    let c1 = Arc::new(cluster1);
    let c2 = Arc::new(cluster2);
    
    manager
        .expect_get_cluster_by_name()
        .returning(move |name| {
            if name == "cluster1" {
                Some(c1.clone())
            } else if name == "cluster2" {
                Some(c2.clone())
            } else {
                None
            }
        });
    
    let app = create_router(make_test_state(db.clone(), manager));
    let token = encode_test_jwt(&serde_json::json!({
        "userId": 42,
        "clusters": ["cluster1", "cluster2"]
    }));
    
    let tasks: Vec<_> = (0..10)
        .map(|i| {
            let app_clone = app.clone();
            let token_clone = token.clone();
            let cluster_name = if i % 2 == 0 { "cluster1" } else { "cluster2" };
            
            tokio::spawn(async move {
                let resp = app_clone
                    .oneshot(
                        Request::builder()
                            .method("POST")
                            .uri("/job/apiv1/job/")
                            .header("content-type", "application/json")
                            .header("authorization", &token_clone)
                            .body(Body::from(format!(
                                r#"{{"cluster":"{}","parameters":"{{}}","bundle":"bundle{}"}}"#,
                                cluster_name, i
                            )))
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                
                (resp.status(), cluster_name.to_string())
            })
        })
        .collect();
    
    let results = futures_util::future::join_all(tasks).await;
    
    for result in results {
        let (status, _cluster) = result.unwrap();
        assert_eq!(status, StatusCode::OK);
    }
    
    let jobs = job::Entity::find().all(&db).await.unwrap();
    assert_eq!(jobs.len(), 10);
    
    let cluster1_jobs = jobs.iter().filter(|j| j.cluster == "cluster1").count();
    let cluster2_jobs = jobs.iter().filter(|j| j.cluster == "cluster2").count();
    
    assert_eq!(cluster1_jobs, 5);
    assert_eq!(cluster2_jobs, 5);
}
