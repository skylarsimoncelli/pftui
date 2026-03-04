use axum::{
    extract::Request,
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::sync::Arc;

#[derive(Clone)]
pub struct AuthState {
    pub token: Option<String>,
}

impl AuthState {
    pub fn new(enabled: bool) -> Self {
        if enabled {
            // Generate a simple random token
            use std::time::{SystemTime, UNIX_EPOCH};
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let token = format!("pftui_{:x}", timestamp);
            println!("🔐 Authentication enabled. Use token: {}", token);
            println!("   Add header: Authorization: Bearer {}", token);
            Self { token: Some(token) }
        } else {
            println!("⚠️  Authentication disabled (--no-auth)");
            Self { token: None }
        }
    }
}

pub async fn auth_middleware(
    state: axum::extract::State<Arc<AuthState>>,
    req: Request,
    next: Next,
) -> Response {
    // If auth is disabled, allow all requests
    if state.token.is_none() {
        return next.run(req).await;
    }

    // Skip auth for static files and root
    let path = req.uri().path();
    if path == "/" || path.starts_with("/static/") {
        return next.run(req).await;
    }

    // Check Authorization header for API routes
    if path.starts_with("/api/") {
        let expected_token = state.token.as_ref().unwrap();
        
        if let Some(auth_header) = req.headers().get(header::AUTHORIZATION) {
            if let Ok(auth_str) = auth_header.to_str() {
                if let Some(token) = auth_str.strip_prefix("Bearer ") {
                    if token == expected_token {
                        return next.run(req).await;
                    }
                }
            }
        }

        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    next.run(req).await
}
