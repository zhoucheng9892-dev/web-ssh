//! Web-ssh account auth: setup, login (with captcha), logout, me, and
//! self-service password change.
//!
//! Sessions are provided by `tower-sessions` (wired in `main.rs` via
//! `SessionManagerLayer`). `Session` is consumed as an extractor here; the
//! current user id is kept under [`USER_ID_KEY`].

pub mod extractors;

use axum::{extract::State, response::IntoResponse, routing::{get, post}, Json};
use serde::{Deserialize, Serialize};
use tower_sessions::Session;

use crate::{
    captcha::{CAPTCHA_KEY, self as captcha},
    db::models::{self, count_users, create_user, find_user_by_name, get_user},
    error::{AppError, AppResult},
    state::AppState,
};

/// Session key holding the authenticated user id.
pub const USER_ID_KEY: &str = "user_id";

#[derive(Debug, Deserialize)]
pub struct Credentials {
    pub username: String,
    pub password: String,
    /// Captcha text from the login form (case-insensitive).
    pub captcha: String,
}

/// Initial-setup payload: username + password only (no captcha, since no
/// session exists yet to hold one and the endpoint is one-shot anyway).
#[derive(Debug, Deserialize)]
pub struct SetupPayload {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct ChangePassword {
    pub old_password: String,
    pub new_password: String,
}

#[derive(Debug, Serialize)]
pub struct Me {
    pub id: i64,
    pub username: String,
    pub is_admin: bool,
}

#[derive(Debug, Serialize)]
pub struct CaptchaImage {
    /// A `data:image/png;base64,...` URL ready for an <img src>.
    pub image: String,
}

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/setup", post(setup))
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/me", get(me))
        .route("/password", post(change_password))
        .route("/captcha", get(captcha_image))
}

/// `POST /api/auth/setup` — create the first admin. Refused once any user exists.
async fn setup(
    State(state): State<AppState>,
    session: Session,
    Json(body): Json<SetupPayload>,
) -> AppResult<impl IntoResponse> {
    if count_users(&state.pool).await? > 0 {
        return Err(AppError::Conflict("setup already completed".into()));
    }
    let creds = Credentials {
        username: body.username,
        password: body.password,
        captcha: String::new(), // setup doesn't require captcha
    };
    validate(&creds)?;
    let hash = crate::crypto::hash_password(&creds.password)?;
    let id = create_user(&state.pool, &creds.username, &hash, true).await?;
    session_insert(&session, id).await?;
    Ok(Json(me_for(&state, id).await?))
}

/// `POST /api/auth/login` — username + password + captcha.
async fn login(
    State(state): State<AppState>,
    session: Session,
    Json(creds): Json<Credentials>,
) -> AppResult<impl IntoResponse> {
    // 1) Verify captcha first (rotate it regardless of outcome so it can't be reused).
    let expected: Option<String> = session.get(CAPTCHA_KEY).await.map_err(session_err)?;
    let _ = session.remove::<String>(CAPTCHA_KEY).await;
    match expected {
        Some(exp) if eq_ignore_case(&creds.captcha, &exp) => {}
        _ => return Err(AppError::BadRequest("验证码错误".into())),
    }

    // 2) Verify credentials.
    let user = find_user_by_name(&state.pool, &creds.username)
        .await?
        .ok_or_else(|| AppError::Unauthorized)?;
    if user.is_disabled {
        return Err(AppError::Forbidden);
    }
    crate::crypto::verify_password(&creds.password, &user.password_hash)
        .map_err(|_| AppError::Unauthorized)?;
    session_insert(&session, user.id).await?;
    Ok(Json(Me {
        id: user.id,
        username: user.username,
        is_admin: user.is_admin,
    }))
}

/// `POST /api/auth/logout`.
async fn logout(session: Session) -> AppResult<impl IntoResponse> {
    session.flush().await.map_err(session_err)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `GET /api/auth/me`.
async fn me(user: crate::auth::extractors::RequireUser) -> AppResult<impl IntoResponse> {
    Ok(Json(Me {
        id: user.0.id,
        username: user.0.username,
        is_admin: user.0.is_admin,
    }))
}

/// `POST /api/auth/password` — self-service password change.
async fn change_password(
    State(state): State<AppState>,
    user: crate::auth::extractors::RequireUser,
    Json(body): Json<ChangePassword>,
) -> AppResult<impl IntoResponse> {
    if body.new_password.len() < 6 {
        return Err(AppError::BadRequest("新密码至少 6 位".into()));
    }
    crate::crypto::verify_password(&body.old_password, &user.0.password_hash)
        .map_err(|_| AppError::BadRequest("原密码错误".into()))?;
    let hash = crate::crypto::hash_password(&body.new_password)?;
    models::update_user_password(&state.pool, user.0.id, &hash).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `GET /api/auth/captcha` — mint a fresh captcha image and store its answer.
async fn captcha_image(session: Session) -> AppResult<Json<CaptchaImage>> {
    let (image, answer) = captcha::generate();
    session
        .insert(CAPTCHA_KEY, answer.to_lowercase())
        .await
        .map_err(session_err)?;
    Ok(Json(CaptchaImage { image }))
}

/// `GET /api/auth/status` — public probe: do we need initial setup?
pub async fn status(State(state): State<AppState>) -> AppResult<impl IntoResponse> {
    let n = count_users(&state.pool).await?;
    Ok(Json(serde_json::json!({ "needs_setup": n == 0 })))
}

fn validate(c: &Credentials) -> AppResult<()> {
    let u = c.username.trim();
    if !(3..=32).contains(&u.len()) {
        return Err(AppError::BadRequest("username must be 3-32 chars".into()));
    }
    if c.password.len() < 6 {
        return Err(AppError::BadRequest("password too short".into()));
    }
    Ok(())
}

async fn session_insert(session: &Session, user_id: i64) -> AppResult<()> {
    session
        .insert(USER_ID_KEY, user_id)
        .await
        .map_err(session_err)
}

fn session_err(e: tower_sessions::session::Error) -> AppError {
    tracing::error!(error = %e, "session error");
    AppError::Other(anyhow::anyhow!("session storage failed"))
}

async fn me_for(state: &AppState, id: i64) -> AppResult<Me> {
    let u = get_user(&state.pool, id)
        .await?
        .ok_or(AppError::Unauthorized)?;
    Ok(Me {
        id: u.id,
        username: u.username,
        is_admin: u.is_admin,
    })
}

fn eq_ignore_case(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}
