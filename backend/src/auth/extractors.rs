//! Request extractors for authentication.
//!
//! - [`AuthUser`]: just the user id, from the session. (Legacy, kept for
//!   terminal/files where we don't need to re-check status.)
//! - [`RequireUser`]: loads the full user and rejects disabled accounts, so a
//!   frozen user cannot use any authenticated endpoint even with a valid cookie.
//! - [`AdminUser`]: like [`RequireUser`] but also requires `is_admin`.

use axum::extract::FromRequestParts;
use tower_sessions::Session;

use crate::{
    auth::USER_ID_KEY,
    db::models::{self, User},
    error::{AppError, AppResult},
    state::AppState,
};

/// The id of the authenticated user, taken from the session.
#[derive(Debug, Clone, Copy)]
pub struct AuthUser(pub i64);

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let uid = session_uid(parts).await?;
        Ok(AuthUser(uid))
    }
}

/// Loads the full user record; rejects if missing or disabled.
#[derive(Debug, Clone)]
pub struct RequireUser(pub User);

impl FromRequestParts<AppState> for RequireUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let uid = session_uid(parts).await?;
        let user = models::get_user(&state.pool, uid)
            .await?
            .ok_or(AppError::Unauthorized)?;
        if user.is_disabled {
            // Frozen account: treat as logged out.
            return Err(AppError::Forbidden);
        }
        Ok(RequireUser(user))
    }
}

/// Requires an enabled admin account.
#[derive(Debug, Clone)]
pub struct AdminUser(pub User);

impl FromRequestParts<AppState> for AdminUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = RequireUser::from_request_parts(parts, state).await?.0;
        if !user.is_admin {
            return Err(AppError::Forbidden);
        }
        Ok(AdminUser(user))
    }
}

/// Read the user id from the session in request extensions.
async fn session_uid(parts: &mut axum::http::request::Parts) -> AppResult<i64> {
    let session = parts
        .extensions
        .get::<Session>()
        .ok_or(AppError::Unauthorized)?
        .clone();
    let uid: Option<i64> = session.get(USER_ID_KEY).await.map_err(|e| {
        tracing::warn!(error = %e, "session read");
        AppError::Unauthorized
    })?;
    uid.ok_or(AppError::Unauthorized)
}

/// Convenience: read the decrypted secret for a connection belonging to the
/// authenticated user.
pub async fn resolve_secret(
    state: &AppState,
    user_id: i64,
    connection_id: i64,
) -> AppResult<models::ConnectionSecret> {
    let row = models::get_connection_secret(&state.pool, user_id, connection_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let (auth_type, enc, iv) = row;
    let secret = crate::crypto::decrypt_secret(&state.config.master_key, &enc, &iv)?;
    Ok(models::ConnectionSecret { auth_type, secret })
}
