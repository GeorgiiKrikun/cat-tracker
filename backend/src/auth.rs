//! Password hashing (argon2id) and cookie sessions stored in SQLite.

use argon2::{
    Argon2,
    password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash},
};
use axum::{extract::FromRequestParts, http::request::Parts};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use chrono::{Duration, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::{AppState, error::AppError, now_ts};

pub const SESSION_COOKIE: &str = "ct_session";
pub const SESSION_DAYS: i64 = 90;
/// Sessions are extended on use once fewer than this many days remain.
const RENEW_BELOW_DAYS: i64 = 60;

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let hash = Argon2::default()
        .hash_password(password.as_bytes())
        .map_err(|e| anyhow::anyhow!("hashing failed: {e}"))?;
    Ok(hash.to_string())
}

pub fn verify_password(password: &str, phc: &str) -> bool {
    match PasswordHash::new(phc) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// A dummy hash used to spend the same time on unknown usernames.
pub fn dummy_verify(password: &str) {
    static DUMMY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let h = DUMMY.get_or_init(|| hash_password("dummy-password").unwrap_or_default());
    let _ = verify_password(password, h);
}

fn token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn new_token() -> String {
    // Two v4 UUIDs = 244 bits from the OS CSPRNG.
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

fn expiry() -> String {
    (Utc::now() + Duration::days(SESSION_DAYS))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn session_cookie(token: String, secure: bool) -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE, token))
        .path("/")
        .http_only(true)
        .secure(secure)
        .same_site(SameSite::Lax)
        .max_age(time::Duration::days(SESSION_DAYS))
        .build()
}

pub fn removal_cookie(secure: bool) -> Cookie<'static> {
    Cookie::build((SESSION_COOKIE, ""))
        .path("/")
        .http_only(true)
        .secure(secure)
        .same_site(SameSite::Lax)
        .max_age(time::Duration::ZERO)
        .build()
}

pub async fn create_session(db: &SqlitePool, user_id: i64) -> sqlx::Result<String> {
    let token = new_token();
    sqlx::query("INSERT INTO sessions (token_hash, user_id, expires_at) VALUES (?, ?, ?)")
        .bind(token_hash(&token))
        .bind(user_id)
        .bind(expiry())
        .execute(db)
        .await?;
    // Opportunistic cleanup of expired sessions.
    sqlx::query("DELETE FROM sessions WHERE expires_at < ?")
        .bind(now_ts())
        .execute(db)
        .await?;
    Ok(token)
}

pub async fn delete_session(db: &SqlitePool, token: &str) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
        .bind(token_hash(token))
        .execute(db)
        .await?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct CurrentUser {
    pub id: i64,
    pub username: String,
    pub display_name: String,
    #[serde(skip)]
    pub expires_at: String,
}

/// Extractor: any handler that takes `CurrentUser` requires a valid session.
impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, AppError> {
        let jar = CookieJar::from_headers(&parts.headers);
        let token = jar
            .get(SESSION_COOKIE)
            .map(|c| c.value().to_string())
            .ok_or(AppError::Unauthorized)?;
        let hash = token_hash(&token);

        let user: CurrentUser = sqlx::query_as(
            "SELECT u.id, u.username, u.display_name, s.expires_at
               FROM sessions s JOIN users u ON u.id = s.user_id
              WHERE s.token_hash = ? AND s.expires_at > ?",
        )
        .bind(&hash)
        .bind(now_ts())
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::Unauthorized)?;

        // Sliding expiry: keep phones logged in as long as the app is used.
        let renew_after = (Utc::now() + Duration::days(RENEW_BELOW_DAYS))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        if user.expires_at < renew_after {
            sqlx::query("UPDATE sessions SET expires_at = ? WHERE token_hash = ?")
                .bind(expiry())
                .bind(&hash)
                .execute(&state.db)
                .await?;
        }
        Ok(user)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_roundtrip() {
        let h = hash_password("correct horse").unwrap();
        assert!(h.starts_with("$argon2id$"));
        assert!(verify_password("correct horse", &h));
        assert!(!verify_password("wrong", &h));
        assert!(!verify_password("x", "not-a-hash"));
    }

    #[test]
    fn tokens_are_unique_and_long() {
        let a = new_token();
        let b = new_token();
        assert_ne!(a, b);
        assert_eq!(a.len(), 64);
        assert_eq!(token_hash(&a).len(), 64);
    }
}
