use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use jsonwebtoken::{Algorithm, DecodingKey, Validation};

use crate::config::access_secrets::AccessSecret;

/// Result of a successful JWT authorization check.
#[derive(Debug, Clone)]
pub struct AuthResult {
    /// Decoded JWT payload (claims).
    pub payload: serde_json::Value,
    /// The access secret that successfully validated the token.
    pub secret: AccessSecret,
}

/// Extract JWT secrets from request extensions.
/// The secrets are injected via middleware/layer on the router.
impl<S> FromRequestParts<S> for AuthResult
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, String);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // Get JWT secrets from request extensions
        let secrets = parts
            .extensions
            .get::<Arc<Vec<AccessSecret>>>()
            .cloned()
            .unwrap_or_else(|| Arc::new(Vec::new()));
        tracing::trace!("AUTH: Loaded {} JWT secrets for validation", secrets.len());

        // Get the authorization header
        let auth_header = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                tracing::debug!("AUTH: No authorization header present");
                (StatusCode::FORBIDDEN, "Not authorized".to_string())
            })?;
        tracing::trace!(
            "AUTH: Authorization header found (length: {})",
            auth_header.len()
        );

        // Accept both "Bearer <jwt>" and bare "<jwt>" for backwards compatibility
        let jwt_str = auth_header.strip_prefix("Bearer ").unwrap_or(auth_header);
        tracing::trace!("AUTH: JWT token extracted (length: {})", jwt_str.len());

        // Try each secret with HS256
        let mut validation = Validation::new(Algorithm::HS256);
        validation.required_spec_claims.clear();
        validation.validate_exp = false;

        for (i, secret) in secrets.iter().enumerate() {
            tracing::trace!("AUTH: Trying secret #{} ('{}')", i + 1, secret.name);
            let key = DecodingKey::from_secret(secret.secret.as_bytes());
            if let Ok(token_data) =
                jsonwebtoken::decode::<serde_json::Value>(jwt_str, &key, &validation)
            {
                tracing::debug!(
                    "AUTH: JWT validated successfully with secret '{}'",
                    secret.name
                );
                tracing::trace!("AUTH: JWT payload: {:?}", token_data.claims);
                return Ok(AuthResult {
                    payload: token_data.claims,
                    secret: secret.clone(),
                });
            }
            tracing::trace!(
                "AUTH: Secret #{} ('{}') failed validation",
                i + 1,
                secret.name
            );
        }

        tracing::warn!("AUTH: JWT validation failed - no matching secret found");
        Err((StatusCode::FORBIDDEN, "Not authorized".to_string()))
    }
}

/// Build the applications list from the secret (the secret's own name + its applications).
#[must_use]
/// Return application names authorized by this access secret.
///
/// Always includes the secret's own `name` plus any additional entries from
/// `applications` (one JWT secret may serve multiple client apps).
#[must_use]
pub fn get_applications(secret: &AccessSecret) -> Vec<String> {
    let mut apps = vec![secret.name.clone()];
    apps.extend(secret.applications.iter().cloned());
    apps
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::get;
    use jsonwebtoken::{EncodingKey, Header};
    use tower::ServiceExt;

    fn make_test_secrets() -> Vec<AccessSecret> {
        vec![
            AccessSecret {
                name: "app1".to_string(),
                secret: "secret_one".to_string(),
                applications: vec!["compas".to_string()],
                clusters: vec!["ozstar".to_string()],
            },
            AccessSecret {
                name: "app2".to_string(),
                secret: "secret_two".to_string(),
                applications: vec!["bilby".to_string()],
                clusters: vec!["nci".to_string()],
            },
        ]
    }

    fn encode_jwt(claims: &serde_json::Value, secret: &str) -> String {
        jsonwebtoken::encode(
            &Header::new(Algorithm::HS256),
            claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap()
    }

    fn test_router(secrets: Vec<AccessSecret>) -> Router {
        Router::new()
            .route(
                "/test",
                get(|auth: AuthResult| async move {
                    serde_json::to_string(&serde_json::json!({
                        "name": auth.secret.name,
                        "payload": auth.payload,
                    }))
                    .unwrap()
                }),
            )
            .layer(axum::Extension(Arc::new(secrets)))
    }

    #[tokio::test]
    async fn test_auth_no_header() {
        let app = test_router(make_test_secrets());
        let resp = app
            .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_auth_invalid_token() {
        let app = test_router(make_test_secrets());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/test")
                    .header("authorization", "invalid.token.here")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_auth_valid_first_secret() {
        let secrets = make_test_secrets();
        let claims = serde_json::json!({"userId": 42});
        let token = encode_jwt(&claims, &secrets[0].secret);

        let app = test_router(secrets);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/test")
                    .header("authorization", &token)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["name"], "app1");
        assert_eq!(json["payload"]["userId"], 42);
    }

    #[tokio::test]
    async fn test_auth_valid_second_secret() {
        let secrets = make_test_secrets();
        let claims = serde_json::json!({"userId": 99});
        let token = encode_jwt(&claims, &secrets[1].secret);

        let app = test_router(secrets);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/test")
                    .header("authorization", &token)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["name"], "app2");
    }

    #[tokio::test]
    async fn test_auth_wrong_secret() {
        let secrets = make_test_secrets();
        let claims = serde_json::json!({"userId": 1});
        let token = encode_jwt(&claims, "totally_wrong_secret");

        let app = test_router(secrets);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/test")
                    .header("authorization", &token)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_get_applications() {
        let secret = AccessSecret {
            name: "myapp".to_string(),
            secret: "s".to_string(),
            applications: vec!["a".to_string(), "b".to_string()],
            clusters: vec![],
        };
        let apps = get_applications(&secret);
        assert_eq!(apps, vec!["myapp", "a", "b"]);
    }

    #[tokio::test]
    async fn test_get_applications_empty() {
        let secret = AccessSecret {
            name: "myapp".to_string(),
            secret: "s".to_string(),
            applications: vec![],
            clusters: vec![],
        };
        let apps = get_applications(&secret);
        assert_eq!(apps, vec!["myapp"]);
    }
}
