//! Admin-only audit log query: list terminal commands with optional filtering
//! by user and time range. All routes require [`AdminUser`].

use axum::{
    extract::State,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::{
    auth::extractors::AdminUser,
    db::models::{self, AuditFilter, AuditLog},
    error::{AppError, AppResult},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(list))
}

#[derive(Deserialize)]
struct ListQuery {
    user_id: Option<i64>,
    /// Inclusive start, e.g. "2024-01-01 00:00:00".
    start: Option<String>,
    /// Inclusive end.
    end: Option<String>,
    #[serde(default = "default_page")]
    page: i64,
    #[serde(default = "default_page_size")]
    page_size: i64,
}

fn default_page() -> i64 {
    1
}
fn default_page_size() -> i64 {
    50
}

#[derive(Serialize)]
struct ListResult {
    logs: Vec<AuditLog>,
    total: i64,
}

async fn list(
    _admin: AdminUser,
    State(state): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<ListQuery>,
) -> AppResult<Json<ListResult>> {
    if q.page < 1 || q.page_size < 1 || q.page_size > 500 {
        return Err(AppError::BadRequest("invalid page/page_size".into()));
    }
    let offset = (q.page - 1) * q.page_size;
    let filter = AuditFilter {
        user_id: q.user_id,
        start: q.start,
        end: q.end,
        limit: q.page_size,
        offset,
    };
    let logs = models::list_audit_logs(&state.pool, &filter).await?;
    let total = models::count_audit_logs(&state.pool, &filter).await?;
    Ok(Json(ListResult { logs, total }))
}
