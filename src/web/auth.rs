use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Json, Response},
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const SESSION_COOKIE_NAME: &str = "pftui_session";
const SESSION_TTL_SECONDS: i64 = 60 * 60 * 8;

#[derive(Clone)]
pub struct AuthState {
    pub enabled: bool,
    login_token: Option<String>,
    sessions: Arc<Mutex<HashMap<String, Session>>>,
    secure_cookies: bool,
}

#[derive(Clone)]
struct Session {
    session_id: String,
    issued_at: chrono::DateTime<Utc>,
    expires_at: chrono::DateTime<Utc>,
    csrf_token: String,
    auth_mode: String,
}

#[derive(Serialize)]
pub struct AuthErrorResponse {
    pub code: String,
    pub message: String,
    pub relogin_required: bool,
}

#[derive(Serialize)]
pub struct SessionResponse {
    pub authenticated: bool,
    pub issued_at: Option<String>,
    pub expires_at: Option<String>,
    pub csrf_token: Option<String>,
    pub auth_mode: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub ok: bool,
    pub issued_at: Option<String>,
    pub expires_at: Option<String>,
    pub csrf_token: Option<String>,
    pub auth_mode: String,
}

#[derive(Serialize)]
pub struct LogoutResponse {
    pub ok: bool,
}

#[derive(Serialize)]
pub struct CsrfResponse {
    pub csrf_token: String,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub token: String,
}

#[derive(Debug)]
pub enum AuthFailure {
    MissingSession,
    InvalidSession,
    ExpiredSession,
    MissingCsrf,
    CsrfMismatch,
}

impl AuthState {
    pub fn new(enabled: bool, bind_addr: &str) -> Self {
        if enabled {
            let token = new_token("login");
            println!("🔐 Authentication enabled.");
            println!("   Login token: {}", token);
            println!("   POST /auth/login with JSON: {{\"token\":\"...\"}}");
            Self {
                enabled: true,
                login_token: Some(token),
                sessions: Arc::new(Mutex::new(HashMap::new())),
                secure_cookies: !is_localhost_bind(bind_addr),
            }
        } else {
            println!("⚠️  Authentication disabled (--no-auth)");
            Self {
                enabled: false,
                login_token: None,
                sessions: Arc::new(Mutex::new(HashMap::new())),
                secure_cookies: false,
            }
        }
    }

    fn create_session(&self) -> Session {
        let issued_at = Utc::now();
        Session {
            session_id: new_token("sid"),
            issued_at,
            expires_at: issued_at + Duration::seconds(SESSION_TTL_SECONDS),
            csrf_token: new_token("csrf"),
            auth_mode: "session".to_string(),
        }
    }

    fn validate_session_cookie(&self, req: &Request) -> Result<Session, AuthFailure> {
        let cookie_header = req
            .headers()
            .get(header::COOKIE)
            .and_then(|v| v.to_str().ok())
            .ok_or(AuthFailure::MissingSession)?;
        let session_id = extract_cookie(cookie_header, SESSION_COOKIE_NAME)
            .ok_or(AuthFailure::MissingSession)?;

        let mut sessions = self.sessions.lock().expect("session mutex poisoned");
        if let Some(session) = sessions.get(&session_id).cloned() {
            if session.expires_at < Utc::now() {
                sessions.remove(&session_id);
                return Err(AuthFailure::ExpiredSession);
            }
            return Ok(session);
        }
        Err(AuthFailure::InvalidSession)
    }

    fn validate_api_request(&self, req: &Request) -> Result<Session, AuthFailure> {
        let session = self.validate_session_cookie(req)?;
        if is_mutating(req.method()) {
            let csrf = req
                .headers()
                .get("X-CSRF-Token")
                .and_then(|v| v.to_str().ok())
                .ok_or(AuthFailure::MissingCsrf)?;
            if csrf != session.csrf_token {
                return Err(AuthFailure::CsrfMismatch);
            }
        }
        Ok(session)
    }

    fn session_cookie_header(&self, session_id: &str) -> String {
        let mut parts = vec![
            format!("{}={}", SESSION_COOKIE_NAME, session_id),
            "Path=/".to_string(),
            "HttpOnly".to_string(),
            "SameSite=Lax".to_string(),
            format!("Max-Age={}", SESSION_TTL_SECONDS),
        ];
        if self.secure_cookies {
            parts.push("Secure".to_string());
        }
        parts.join("; ")
    }

    fn clear_cookie_header(&self) -> String {
        let mut parts = vec![
            format!("{}=", SESSION_COOKIE_NAME),
            "Path=/".to_string(),
            "HttpOnly".to_string(),
            "SameSite=Lax".to_string(),
            "Max-Age=0".to_string(),
        ];
        if self.secure_cookies {
            parts.push("Secure".to_string());
        }
        parts.join("; ")
    }
}

pub async fn auth_middleware(
    State(state): State<Arc<AuthState>>,
    req: Request,
    next: Next,
) -> Response {
    if !state.enabled {
        return next.run(req).await;
    }

    let path = req.uri().path();
    if path == "/" || path.starts_with("/static/") || path.starts_with("/auth/") {
        return next.run(req).await;
    }

    if path.starts_with("/api/") {
        if let Err(failure) = state.validate_api_request(&req) {
            let (status, payload) = auth_failure_response(failure);
            return (status, payload).into_response();
        }
    }

    next.run(req).await
}

pub async fn login(
    State(state): State<Arc<AuthState>>,
    Json(body): Json<LoginRequest>,
) -> Result<(HeaderMap, Json<LoginResponse>), (StatusCode, Json<AuthErrorResponse>)> {
    if !state.enabled {
        return Ok((
            HeaderMap::new(),
            Json(LoginResponse {
                ok: true,
                issued_at: None,
                expires_at: None,
                csrf_token: None,
                auth_mode: "no-auth".to_string(),
            }),
        ));
    }

    let Some(expected) = state.login_token.as_ref() else {
        return Err(auth_failure_response(AuthFailure::InvalidSession));
    };
    if body.token != *expected {
        return Err(auth_failure_response(AuthFailure::InvalidSession));
    }

    let session = state.create_session();
    let mut sessions = state.sessions.lock().expect("session mutex poisoned");
    sessions.insert(session.session_id.clone(), session.clone());
    drop(sessions);

    let mut headers = HeaderMap::new();
    headers.insert(
        header::SET_COOKIE,
        state
            .session_cookie_header(&session.session_id)
            .parse()
            .expect("valid set-cookie header"),
    );

    Ok((
        headers,
        Json(LoginResponse {
            ok: true,
            issued_at: Some(session.issued_at.to_rfc3339()),
            expires_at: Some(session.expires_at.to_rfc3339()),
            csrf_token: Some(session.csrf_token),
            auth_mode: session.auth_mode,
        }),
    ))
}

pub async fn logout(
    State(state): State<Arc<AuthState>>,
    req: Request,
) -> (HeaderMap, Json<LogoutResponse>) {
    if state.enabled {
        if let Some(cookie_header) = req.headers().get(header::COOKIE).and_then(|v| v.to_str().ok()) {
            if let Some(session_id) = extract_cookie(cookie_header, SESSION_COOKIE_NAME) {
                let mut sessions = state.sessions.lock().expect("session mutex poisoned");
                sessions.remove(&session_id);
            }
        }
    }

    let mut headers = HeaderMap::new();
    headers.insert(
        header::SET_COOKIE,
        state
            .clear_cookie_header()
            .parse()
            .expect("valid clear-cookie header"),
    );
    (headers, Json(LogoutResponse { ok: true }))
}

pub async fn get_session(
    State(state): State<Arc<AuthState>>,
    req: Request,
) -> Result<Json<SessionResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    if !state.enabled {
        return Ok(Json(SessionResponse {
            authenticated: true,
            issued_at: None,
            expires_at: None,
            csrf_token: None,
            auth_mode: "no-auth".to_string(),
        }));
    }

    let session = state
        .validate_session_cookie(&req)
        .map_err(auth_failure_response)?;
    Ok(Json(SessionResponse {
        authenticated: true,
        issued_at: Some(session.issued_at.to_rfc3339()),
        expires_at: Some(session.expires_at.to_rfc3339()),
        csrf_token: Some(session.csrf_token),
        auth_mode: session.auth_mode,
    }))
}

pub async fn get_csrf(
    State(state): State<Arc<AuthState>>,
    req: Request,
) -> Result<Json<CsrfResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    if !state.enabled {
        return Ok(Json(CsrfResponse {
            csrf_token: String::new(),
        }));
    }
    let session = state
        .validate_session_cookie(&req)
        .map_err(auth_failure_response)?;
    Ok(Json(CsrfResponse {
        csrf_token: session.csrf_token,
    }))
}

pub fn auth_failure_response(
    failure: AuthFailure,
) -> (StatusCode, Json<AuthErrorResponse>) {
    let (status, code, message, relogin_required) = match failure {
        AuthFailure::MissingSession => (
            StatusCode::UNAUTHORIZED,
            "session_missing",
            "Authentication required",
            true,
        ),
        AuthFailure::InvalidSession => (
            StatusCode::UNAUTHORIZED,
            "session_invalid",
            "Session is invalid",
            true,
        ),
        AuthFailure::ExpiredSession => (
            StatusCode::UNAUTHORIZED,
            "session_expired",
            "Session expired",
            true,
        ),
        AuthFailure::MissingCsrf => (
            StatusCode::FORBIDDEN,
            "csrf_missing",
            "Missing CSRF token",
            false,
        ),
        AuthFailure::CsrfMismatch => (
            StatusCode::FORBIDDEN,
            "csrf_mismatch",
            "Invalid CSRF token",
            false,
        ),
    };
    (
        status,
        Json(AuthErrorResponse {
            code: code.to_string(),
            message: message.to_string(),
            relogin_required,
        }),
    )
}

fn new_token(prefix: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let now_ns = Utc::now().timestamp_nanos_opt().unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}_{now_ns:x}_{n:x}")
}

fn is_mutating(method: &Method) -> bool {
    matches!(*method, Method::POST | Method::PUT | Method::PATCH | Method::DELETE)
}

fn extract_cookie(cookie_header: &str, name: &str) -> Option<String> {
    for part in cookie_header.split(';') {
        let trimmed = part.trim();
        if let Some((k, v)) = trimmed.split_once('=') {
            if k == name {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn is_localhost_bind(bind_addr: &str) -> bool {
    matches!(bind_addr, "127.0.0.1" | "localhost" | "::1" | "0.0.0.0")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_cookie_value() {
        let value = extract_cookie("a=1; pftui_session=abc123; b=2", SESSION_COOKIE_NAME);
        assert_eq!(value.as_deref(), Some("abc123"));
    }

    #[test]
    fn extract_cookie_missing() {
        let value = extract_cookie("a=1; b=2", SESSION_COOKIE_NAME);
        assert!(value.is_none());
    }

    #[test]
    fn mutating_methods_require_csrf() {
        assert!(is_mutating(&Method::POST));
        assert!(is_mutating(&Method::PATCH));
        assert!(!is_mutating(&Method::GET));
    }

    #[test]
    fn local_bind_disables_secure_cookie() {
        assert!(is_localhost_bind("127.0.0.1"));
        assert!(is_localhost_bind("0.0.0.0"));
        assert!(!is_localhost_bind("192.168.1.10"));
    }

    #[test]
    fn auth_failure_shapes() {
        let (status, body) = auth_failure_response(AuthFailure::ExpiredSession);
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body.0.code, "session_expired");
        assert!(body.0.relogin_required);
    }
}
