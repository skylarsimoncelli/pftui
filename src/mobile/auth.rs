use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use argon2::{Argon2, PasswordHash, PasswordVerifier};
use axum::{
    extract::{ConnectInfo, Request, State},
    http::{header, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Json, Response},
};
use chrono::{Duration, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;

use crate::config::{MobileApiToken, MobileTokenPermission};

/// Per-IP rate-limiting state for authentication failures.
#[derive(Clone)]
struct RateLimitEntry {
    fail_count: u32,
    blocked_until: Option<chrono::DateTime<Utc>>,
    last_attempt: chrono::DateTime<Utc>,
}

/// Cached session entry: validated grant + expiry timestamp.
#[derive(Clone)]
struct SessionCacheEntry {
    grant: AccessGrant,
    expires_at: chrono::DateTime<Utc>,
}

type SessionCache = Arc<RwLock<HashMap<String, SessionCacheEntry>>>;
type RateLimitMap = Arc<RwLock<HashMap<IpAddr, RateLimitEntry>>>;

#[derive(Clone)]
pub struct MobileAuthState {
    tokens: Vec<MobileApiToken>,
    session_ttl: Duration,
    /// Cache of SHA-256(token) -> SessionCacheEntry for validated tokens only.
    session_cache: SessionCache,
    /// Per-IP rate limiting for auth failures.
    rate_limits: RateLimitMap,
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
            session_cache: Arc::new(RwLock::new(HashMap::new())),
            rate_limits: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Check if an IP is currently rate-limited. Returns `true` if blocked.
    async fn is_rate_limited(&self, ip: IpAddr) -> bool {
        let limits = self.rate_limits.read().await;
        if let Some(entry) = limits.get(&ip) {
            if let Some(blocked_until) = entry.blocked_until {
                if Utc::now() < blocked_until {
                    return true;
                }
            }
        }
        false
    }

    /// Record a failed authentication attempt for rate limiting.
    async fn record_auth_failure(&self, ip: IpAddr) {
        let mut limits = self.rate_limits.write().await;
        let now = Utc::now();

        // Clean up expired entries (older than 1 hour with no recent activity)
        let cutoff = now - Duration::hours(1);
        limits.retain(|_, entry| entry.last_attempt >= cutoff);

        let entry = limits.entry(ip).or_insert(RateLimitEntry {
            fail_count: 0,
            blocked_until: None,
            last_attempt: now,
        });

        entry.fail_count += 1;
        entry.last_attempt = now;

        // After 10 failures, apply escalating block duration
        if entry.fail_count >= 10 {
            let block_minutes = match entry.fail_count {
                10..=14 => 1,
                15..=19 => 5,
                20..=29 => 15,
                _ => 60,
            };
            entry.blocked_until = Some(now + Duration::minutes(block_minutes));
            eprintln!(
                "[mobile-api] Rate limit: IP {} blocked for {}m after {} failures",
                ip, block_minutes, entry.fail_count
            );
        }
    }

    /// Clear rate limit state for an IP on successful auth.
    async fn clear_rate_limit(&self, ip: IpAddr) {
        let mut limits = self.rate_limits.write().await;
        limits.remove(&ip);
    }

    async fn validate_bearer(&self, bearer_token: &str) -> Option<AccessGrant> {
        // Compute SHA-256 hash of the token for cache lookup (never store raw token)
        let token_hash = {
            let mut hasher = Sha256::new();
            hasher.update(bearer_token.as_bytes());
            hex::encode(hasher.finalize())
        };

        // Check session cache first (avoids repeated Argon2 computation)
        {
            let cache = self.session_cache.read().await;
            if let Some(entry) = cache.get(&token_hash) {
                if Utc::now() < entry.expires_at {
                    return Some(entry.grant.clone());
                }
            }
        }

        // Fall back to Argon2 verification
        for token in &self.tokens {
            let Ok(parsed) = PasswordHash::new(&token.token_hash) else {
                continue;
            };
            if Argon2::default()
                .verify_password(bearer_token.as_bytes(), &parsed)
                .is_ok()
            {
                let grant = AccessGrant {
                    permission: token.permission,
                };

                // Cache the validated session using SHA-256 hash as key
                let mut cache = self.session_cache.write().await;
                let cutoff = Utc::now() - self.session_ttl;
                cache.retain(|_, entry| entry.expires_at >= cutoff);
                cache.insert(
                    token_hash,
                    SessionCacheEntry {
                        grant: grant.clone(),
                        expires_at: Utc::now() + self.session_ttl,
                    },
                );

                return Some(grant);
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
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path();
    if path == "/health" {
        return next.run(req).await;
    }

    let client_ip = addr.ip();

    // Check rate limiting before processing auth
    if state.is_rate_limited(client_ip).await {
        eprintln!(
            "[mobile-api] Auth rejected (rate limited): {}",
            client_ip
        );
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(ErrorResponse {
                code: "rate_limited",
                message: "Too many failed attempts. Try again later.",
            }),
        )
            .into_response();
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
        state.record_auth_failure(client_ip).await;
        eprintln!("[mobile-api] Auth failed from {}", client_ip);
        return (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                code: "unauthorized",
                message: "Missing or invalid API token.",
            }),
        )
            .into_response();
    };

    // Clear rate limit on successful auth
    state.clear_rate_limit(client_ip).await;

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

    fn make_test_state() -> MobileAuthState {
        let salt = SaltString::encode_b64(b"0123456789abcdef").unwrap();
        let hash = Argon2::default()
            .hash_password("pftm_test".as_bytes(), &salt)
            .unwrap()
            .to_string();
        MobileAuthState::new(
            vec![MobileApiToken {
                name: "ios".to_string(),
                prefix: "pftm_read_1234".to_string(),
                token_hash: hash,
                permission: MobileTokenPermission::Read,
                created_at: "2026-03-16T00:00:00Z".to_string(),
            }],
            12,
        )
    }

    #[tokio::test]
    async fn verifies_matching_api_token() {
        let state = make_test_state();
        assert!(state.validate_bearer("pftm_test").await.is_some());
        assert!(state.validate_bearer("bad").await.is_none());
    }

    #[tokio::test]
    async fn session_cache_avoids_repeated_argon2() {
        let state = make_test_state();
        // First call does full Argon2 verification
        assert!(state.validate_bearer("pftm_test").await.is_some());
        // Second call should hit the SHA-256 session cache
        assert!(state.validate_bearer("pftm_test").await.is_some());
        // Cache should have exactly one entry
        let cache = state.session_cache.read().await;
        assert_eq!(cache.len(), 1);
    }

    #[tokio::test]
    async fn rate_limiting_blocks_after_threshold() {
        let state = make_test_state();
        let ip = std::net::IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 1, 100));

        // Should not be rate limited initially
        assert!(!state.is_rate_limited(ip).await);

        // Record 10 failures
        for _ in 0..10 {
            state.record_auth_failure(ip).await;
        }

        // Should now be rate limited
        assert!(state.is_rate_limited(ip).await);
    }

    #[tokio::test]
    async fn successful_auth_clears_rate_limit() {
        let state = make_test_state();
        let ip = std::net::IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 1, 101));

        for _ in 0..10 {
            state.record_auth_failure(ip).await;
        }
        assert!(state.is_rate_limited(ip).await);

        state.clear_rate_limit(ip).await;
        assert!(!state.is_rate_limited(ip).await);
    }
}
