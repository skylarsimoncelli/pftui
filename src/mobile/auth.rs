use std::collections::HashMap;
use std::sync::Arc;

use argon2::{Argon2, PasswordHash, PasswordVerifier};
use axum::{
    extract::{Request, State},
    http::{header, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Json, Response},
};
use chrono::{Duration, Utc};
use serde::Serialize;
use tokio::sync::RwLock;

use crate::config::{MobileApiToken, MobileTokenPermission};

#[derive(Clone)]
pub struct MobileAuthState {
    tokens: Vec<MobileApiToken>,
    session_ttl: Duration,
    seen_sessions: Arc<RwLock<HashMap<String, chrono::DateTime<Utc>>>>,
}

#[derive(Clone)]
struct AccessGrant {
    permission: MobileTokenPermission,
}

#[derive(Serialize)]
pub struct HealthResponse {
    pub ok: bool,
    pub auth_required: bool,
}

#[derive(Serialize)]
pub struct ErrorResponse {
    code: &'static str,
    message: &'static str,
}

impl MobileAuthState {
    pub fn new(tokens: Vec<MobileApiToken>, session_ttl_hours: u64) -> Self {
        Self {
            tokens,
            session_ttl: Duration::hours(session_ttl_hours.max(1) as i64),
            seen_sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn validate_bearer(&self, bearer_token: &str) -> Option<AccessGrant> {
        let mut sessions = self.seen_sessions.write().await;
        let cutoff = Utc::now() - self.session_ttl;
        sessions.retain(|_, seen_at| *seen_at >= cutoff);
        sessions.insert(bearer_token.to_string(), Utc::now());
        drop(sessions);

        for token in &self.tokens {
            let Ok(parsed) = PasswordHash::new(&token.token_hash) else {
                continue;
            };
            if Argon2::default()
                .verify_password(bearer_token.as_bytes(), &parsed)
                .is_ok()
            {
                return Some(AccessGrant {
                    permission: token.permission,
                });
            }
        }
        None
    }
}

pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        ok: true,
        auth_required: true,
    })
}

pub async fn auth_middleware(
    State(state): State<Arc<MobileAuthState>>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path();
    if path == "/health" {
        return next.run(req).await;
    }

    let auth_header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();

    let token = auth_header
        .strip_prefix("Bearer ")
        .map(str::trim)
        .unwrap_or_default();

    let Some(grant) = state.validate_bearer(token).await else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                code: "unauthorized",
                message: "Missing or invalid API token.",
            }),
        )
            .into_response();
    };

    if is_write_request(req.method()) && grant.permission != MobileTokenPermission::Write {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                code: "insufficient_scope",
                message: "This API token does not have write permission.",
            }),
        )
            .into_response();
    }

    next.run(req).await
}

fn is_write_request(method: &Method) -> bool {
    !matches!(method, &Method::GET | &Method::HEAD | &Method::OPTIONS)
}

#[cfg(test)]
mod tests {
    use super::MobileAuthState;
    use crate::config::{MobileApiToken, MobileTokenPermission};
    use argon2::{password_hash::SaltString, Argon2, PasswordHasher};

    #[tokio::test]
    async fn verifies_matching_api_token() {
        let salt = SaltString::encode_b64(b"0123456789abcdef").unwrap();
        let hash = Argon2::default()
            .hash_password("pftm_test".as_bytes(), &salt)
            .unwrap()
            .to_string();
        let state = MobileAuthState::new(
            vec![MobileApiToken {
                name: "ios".to_string(),
                prefix: "pftm_read_1234".to_string(),
                token_hash: hash,
                permission: MobileTokenPermission::Read,
                created_at: "2026-03-16T00:00:00Z".to_string(),
            }],
            12,
        );
        assert!(state.validate_bearer("pftm_test").await.is_some());
        assert!(state.validate_bearer("bad").await.is_none());
    }
}
