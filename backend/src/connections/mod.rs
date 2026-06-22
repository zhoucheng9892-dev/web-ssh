//! SSH connection profile CRUD.
//!
//! Secrets (password or PEM key) are encrypted with AES-256-GCM before being
//! stored; never returned to clients. All access is scoped to the requesting
//! user via `user_id`.

use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use serde::Serialize;
use tracing::warn;

use crate::{
    auth::extractors::AuthUser,
    db::models::{
        Connection, ConnectionInput, delete_connection, insert_connection,
        list_connections, update_connection,
    },
    error::{AppError, AppResult},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_all).post(create))
        .route("/{id}", get(detail).put(update_one).delete(delete_one))
}

async fn list_all(State(state): State<AppState>, user: AuthUser) -> AppResult<Json<Vec<Connection>>> {
    Ok(Json(list_connections(&state.pool, user.0).await?))
}

/// Subset of [`ConnectionInput`] safe to echo back (no secret).
#[derive(Serialize)]
struct DetailOut {
    id: i64,
    name: String,
    host: String,
    port: i16,
    username: String,
    auth_type: String,
}

async fn detail(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> AppResult<Json<DetailOut>> {
    let conns = list_connections(&state.pool, user.0).await?;
    let c = conns
        .into_iter()
        .find(|c| c.id == id)
        .ok_or(AppError::NotFound)?;
    Ok(Json(DetailOut {
        id: c.id,
        name: c.name,
        host: c.host,
        port: c.port,
        username: c.username,
        auth_type: c.auth_type,
    }))
}

async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Json(input): Json<ConnectionInput>,
) -> AppResult<Json<serde_json::Value>> {
    validate(&input)?;
    let secret = input
        .secret
        .as_ref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::BadRequest("secret is required".into()))?;
    let (enc, iv) = crate::crypto::encrypt_secret(&state.config.master_key, secret.as_bytes())?;
    let id = insert_connection(&state.pool, user.0, &input, &enc, &iv).await?;
    Ok(Json(serde_json::json!({ "id": id })))
}

async fn update_one(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
    Json(input): Json<ConnectionInput>,
) -> AppResult<Json<serde_json::Value>> {
    validate(&input)?;
    let encrypted = match input.secret.as_ref().filter(|s| !s.is_empty()) {
        Some(s) => {
            let (enc, iv) = crate::crypto::encrypt_secret(&state.config.master_key, s.as_bytes())?;
            Some((enc, iv))
        }
        None => None, // keep existing secret
    };
    let encrypted_ref = encrypted
        .as_ref()
        .map(|(c, n)| (c.as_slice(), n.as_slice()));
    let updated = update_connection(&state.pool, user.0, id, &input, encrypted_ref).await?;
    if !updated {
        return Err(AppError::NotFound);
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn delete_one(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    if !delete_connection(&state.pool, user.0, id).await? {
        return Err(AppError::NotFound);
    }
    // Drop any cached SFTP session for this connection.
    {
        let mut map = state.sftp.write().await;
        map.remove(&(user.0, id));
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

fn validate(input: &ConnectionInput) -> AppResult<()> {
    if input.name.trim().is_empty() {
        return Err(AppError::BadRequest("name is required".into()));
    }
    if input.host.trim().is_empty() {
        return Err(AppError::BadRequest("host is required".into()));
    }
    if input.username.trim().is_empty() {
        return Err(AppError::BadRequest("username is required".into()));
    }
    match input.auth_type.as_str() {
        "password" | "key" => {}
        other => {
            warn!(auth_type = other, "invalid auth_type");
            return Err(AppError::BadRequest("auth_type must be 'password' or 'key'".into()));
        }
    }
    Ok(())
}
