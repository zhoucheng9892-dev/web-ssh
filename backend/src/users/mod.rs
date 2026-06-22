//! Admin-only user management: list, create, delete, freeze/unfreeze,
//! reset password, toggle admin. All routes require [`AdminUser`].
//!
//! Safety rules:
//! - An admin cannot target themselves for delete/freeze/demote (avoids
//!   locking the only admin out).
//! - The last remaining admin cannot be deleted or demoted.

use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use tracing::warn;

use crate::{
    auth::extractors::AdminUser,
    db::models::{self, UserInfo},
    error::{AppError, AppResult},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", get(detail).delete(remove).put(update))
        .route("/{id}/password", post(reset_password))
        .route("/{id}/freeze", post(freeze))
        .route("/{id}/unfreeze", post(unfreeze))
}

#[derive(Deserialize)]
struct CreateUser {
    username: String,
    password: String,
    is_admin: Option<bool>,
}

#[derive(Deserialize)]
struct UpdateUser {
    is_admin: Option<bool>,
}

#[derive(Deserialize)]
struct ResetPassword {
    password: String,
}

async fn list(_admin: AdminUser, State(state): State<AppState>) -> AppResult<Json<Vec<UserInfo>>> {
    Ok(Json(models::list_users(&state.pool).await?))
}

async fn detail(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<UserInfo>> {
    let users = models::list_users(&state.pool).await?;
    users
        .into_iter()
        .find(|u| u.id == id)
        .map(Json)
        .ok_or(AppError::NotFound)
}

async fn create(
    _admin: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<CreateUser>,
) -> AppResult<Json<serde_json::Value>> {
    let u = body.username.trim();
    if !(3..=32).contains(&u.len()) {
        return Err(AppError::BadRequest("用户名需 3-32 字符".into()));
    }
    if body.password.len() < 6 {
        return Err(AppError::BadRequest("密码至少 6 位".into()));
    }
    let hash = crate::crypto::hash_password(&body.password)?;
    let id = models::create_user(&state.pool, u, &hash, body.is_admin.unwrap_or(false)).await?;
    Ok(Json(serde_json::json!({ "id": id })))
}

async fn remove(
    admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    if id == admin.0.id {
        return Err(AppError::BadRequest("不能删除自己".into()));
    }
    guard_last_admin(&state, id, false).await?;
    if !models::delete_user(&state.pool, id).await? {
        return Err(AppError::NotFound);
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn update(
    admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateUser>,
) -> AppResult<Json<serde_json::Value>> {
    if let Some(is_admin) = body.is_admin {
        // Demoting: protect the last admin (and self, to avoid self-lockout).
        if !is_admin {
            if id == admin.0.id {
                return Err(AppError::BadRequest("不能取消自己的管理员身份".into()));
            }
            guard_last_admin(&state, id, true).await?;
        }
        if !models::set_user_admin(&state.pool, id, is_admin).await? {
            return Err(AppError::NotFound);
        }
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn reset_password(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<ResetPassword>,
) -> AppResult<Json<serde_json::Value>> {
    if body.password.len() < 6 {
        return Err(AppError::BadRequest("密码至少 6 位".into()));
    }
    let hash = crate::crypto::hash_password(&body.password)?;
    if !models::update_user_password(&state.pool, id, &hash).await? {
        return Err(AppError::NotFound);
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn freeze(admin: AdminUser, State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Json<serde_json::Value>> {
    if id == admin.0.id {
        return Err(AppError::BadRequest("不能冻结自己".into()));
    }
    if !models::set_user_disabled(&state.pool, id, true).await? {
        return Err(AppError::NotFound);
    }
    // Force the user offline: a subsequent request from them will be rejected
    // by the RequireUser extractor because is_disabled is now set.
    warn!(target = id, "user frozen by admin {}", admin.0.id);
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn unfreeze(_admin: AdminUser, State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Json<serde_json::Value>> {
    if !models::set_user_disabled(&state.pool, id, false).await? {
        return Err(AppError::NotFound);
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// Reject operations that would leave zero admins: if `id` is an admin and is
/// the only admin, return Conflict. `demoting` distinguishes delete vs demote
/// messaging.
async fn guard_last_admin(state: &AppState, id: i64, demoting: bool) -> AppResult<()> {
    let users = models::list_users(&state.pool).await?;
    let target = users.iter().find(|u| u.id == id);
    let is_admin = target.map(|u| u.is_admin).unwrap_or(false);
    if is_admin {
        let admin_count = users.iter().filter(|u| u.is_admin).count();
        if admin_count <= 1 {
            let action = if demoting { "降级" } else { "删除" };
            return Err(AppError::Conflict(format!(
                "不能{action}最后一个管理员"
            )));
        }
    }
    Ok(())
}
