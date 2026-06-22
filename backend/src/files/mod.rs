//! SFTP file management: list, stat, download (streaming), upload (multipart),
//! delete file/dir, mkdir.
//!
//! SFTP sessions are cached per (user_id, connection_id) so repeated requests
//! reuse one subsystem channel. The cache lives in [`AppState::sftp`].

use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Multipart, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tracing::warn;

use crate::{
    auth::extractors::{AuthUser, resolve_secret},
    db::models::{get_connection_dial, touch_last_used},
    error::{AppError, AppResult},
    state::AppState,
};

/// A cached SFTP session. `Mutex` serialises operations on the single
/// subsystem channel; SFTP is request/response so that's fine.
type CachedSftp = Arc<Mutex<Option<russh_sftp::client::SftpSession>>>;

#[derive(Deserialize)]
#[allow(dead_code)] // used by axum query extraction
struct ConnQuery {
    connection_id: i64,
}

#[derive(Deserialize)]
pub struct PathQuery {
    pub connection_id: i64,
    /// Remote directory to list, or remote file path for stat. Defaults to ".".
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Serialize)]
struct Entry {
    name: String,
    is_dir: bool,
    size: u64,
    modified: Option<String>,
}

/// `GET /api/files/list?connection_id=&path=`
pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Query(q): Query<PathQuery>,
) -> AppResult<impl IntoResponse> {
    let path = q.path.clone().unwrap_or_else(|| ".".to_string());
    let sftp = get_sftp(&state, user.0, q.connection_id).await?;
    let mut guard = sftp.lock().await;
    let sftp = guard.as_mut().unwrap();

    let entries = sftp
        .read_dir(&path)
        .await
        .map_err(|e| AppError::BadRequest(format!("read_dir: {e}")))?;
    let mut out: Vec<Entry> = entries
        .filter_map(|e| {
            let meta = e.metadata();
            Some(Entry {
                name: e.file_name(),
                is_dir: meta.is_dir(),
                size: meta.len(),
                modified: meta.modified().ok().map(|t| {
                    let dt: chrono::DateTime<chrono::Utc> = t.into();
                    dt.to_rfc3339()
                }),
            })
        })
        .collect();
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
    Ok(Json(serde_json::json!({ "path": path, "entries": out })))
}

/// `GET /api/files/download?connection_id=&path=` — streamed file download.
pub async fn download(
    State(state): State<AppState>,
    user: AuthUser,
    Query(q): Query<PathQuery>,
) -> AppResult<Response> {
    let path = q
        .path
        .clone()
        .filter(|p| !p.is_empty())
        .ok_or_else(|| AppError::BadRequest("path is required".into()))?;
    let filename = std::path::Path::new(&path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("download")
        .to_string();

    let sftp = get_sftp(&state, user.0, q.connection_id).await?;
    let mut guard = sftp.lock().await;
    let sftp = guard.as_mut().unwrap();

    let mut file = sftp
        .open(&path)
        .await
        .map_err(|e| AppError::BadRequest(format!("open: {e}")))?;

    // Stream the whole file through a channel so we can release the lock and
    // send headers immediately.
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<axum::body::Bytes, std::io::Error>>(8);
    let path_label = filename.clone();
    tokio::spawn(async move {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match file.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    if tx.send(Ok(axum::body::Bytes::copy_from_slice(&buf[..n]))).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(e)).await;
                    break;
                }
            }
        }
        drop(path_label);
    });
    drop(guard);

    let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    let body = Body::from_stream(stream);
    let cd = format!("attachment; filename=\"{}\"", filename.replace('"', ""));

    Ok((
        StatusCode::OK,
        [(header::CONTENT_DISPOSITION, cd.as_str())],
        body,
    )
        .into_response())
}

/// `POST /api/files/upload?connection_id=&path=` — multipart upload.
///
/// Accepts one or more files; each part is written to `path/<filename>`.
pub async fn upload(
    State(state): State<AppState>,
    user: AuthUser,
    Query(q): Query<PathQuery>,
    mut multipart: Multipart,
) -> AppResult<impl IntoResponse> {
    let dir = q.path.clone().unwrap_or_else(|| ".".to_string());
    // Cache the SFTP handle up front so we can lock+write inside the loop while
    // the multipart is borrowed; the Arc clone lets us hand it into each chunk.
    let sftp = get_sftp(&state, user.0, q.connection_id).await?;
    let mut uploaded: Vec<String> = Vec::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("multipart: {e}")))?
    {
        let name = field
            .file_name()
            .or_else(|| field.name())
            .ok_or_else(|| AppError::BadRequest("missing file name".into()))?
            .to_string();
        let target = join_remote(&dir, &name);

        // Create the remote file under a short lock, then stream chunks.
        {
            let mut guard = sftp.lock().await;
            let s = guard.as_mut().unwrap();
            let remote = s
                .create(&target)
                .await
                .map_err(|e| AppError::BadRequest(format!("create {target}: {e}")))?;
            stream_field_to(remote, field, &target).await?;
        }
        uploaded.push(target);
    }

    Ok(Json(serde_json::json!({ "uploaded": uploaded })))
}

/// Drain a multipart field into an open SFTP file using `AsyncWrite`.
///
/// Held while the SFTP cache lock is taken by the caller so we serialise
/// subsystem access across concurrent uploads to the same connection.
async fn stream_field_to(
    mut remote: russh_sftp::client::fs::File,
    mut field: axum::extract::multipart::Field<'_>,
    label: &str,
) -> AppResult<()> {
    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|e| AppError::BadRequest(format!("read field: {e}")))?
    {
        remote
            .write_all(&chunk)
            .await
            .map_err(|e| AppError::BadRequest(format!("write {label}: {e}")))?;
    }
    remote
        .flush()
        .await
        .map_err(|e| AppError::BadRequest(format!("flush {label}: {e}")))?;
    // File closes on drop.
    Ok(())
}

/// `POST /api/files/mkdir?connection_id=&path=`
pub async fn mkdir(
    State(state): State<AppState>,
    user: AuthUser,
    Query(q): Query<PathQuery>,
) -> AppResult<impl IntoResponse> {
    let path = q
        .path
        .clone()
        .filter(|p| !p.is_empty())
        .ok_or_else(|| AppError::BadRequest("path is required".into()))?;
    let sftp = get_sftp(&state, user.0, q.connection_id).await?;
    let guard = sftp.lock().await;
    let sftp = guard.as_ref().unwrap();
    sftp.create_dir(&path)
        .await
        .map_err(|e| AppError::BadRequest(format!("mkdir: {e}")))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// `DELETE /api/files?connection_id=&path=` — delete a file or empty directory.
pub async fn remove(
    State(state): State<AppState>,
    user: AuthUser,
    Query(q): Query<PathQuery>,
) -> AppResult<impl IntoResponse> {
    let path = q
        .path
        .clone()
        .filter(|p| !p.is_empty())
        .ok_or_else(|| AppError::BadRequest("path is required".into()))?;
    let sftp = get_sftp(&state, user.0, q.connection_id).await?;
    let guard = sftp.lock().await;
    let sftp = guard.as_ref().unwrap();

    // Try file first, fall back to directory removal.
    if let Err(file_err) = sftp.remove_file(&path).await {
        match sftp.remove_dir(&path).await {
            Ok(()) => {}
            Err(dir_err) => {
                warn!(?file_err, ?dir_err, %path, "remove failed");
                return Err(AppError::BadRequest(format!(
                    "remove: file [{file_err}] dir [{dir_err}]"
                )));
            }
        }
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

// --- SFTP session cache -----------------------------------------------------

/// Resolve the cached SFTP session for a connection, opening one if needed.
async fn get_sftp(state: &AppState, user_id: i64, connection_id: i64) -> AppResult<CachedSftp> {
    // Cache miss: open a new SSH connection + SFTP session and store it.
    let key = (user_id, connection_id);
    {
        let map = state.sftp.read().await;
        if let Some(s) = map.get(&key) {
            return Ok(s.clone());
        }
    }
    // Resolve credentials + dial info.
    let secret = resolve_secret(state, user_id, connection_id).await?;
    let dial = get_connection_dial(&state.pool, user_id, connection_id)
        .await?
        .ok_or(AppError::NotFound)?;
    // SSH dial + SFTP setup failures are surfaced as 400 with the real cause so
    // the user sees "connection refused" / "auth rejected" rather than a 500.
    let handle = crate::ssh::connect(&dial.host, dial.port as u16, &dial.username, &secret)
        .await
        .map_err(|e| AppError::BadRequest(format!("SSH 连接失败: {e}")))?;
    let sftp = crate::ssh::open_sftp(&handle)
        .await
        .map_err(|e| AppError::BadRequest(format!("SFTP 子系统初始化失败: {e}")))?;
    let _ = touch_last_used(&state.pool, connection_id).await;
    let cached = Arc::new(Mutex::new(Some(sftp)));
    let mut map = state.sftp.write().await;
    map.insert(key, cached.clone());
    Ok(cached)
}

/// Join a remote directory and a filename using `/`. POSIX rules on the wire.
fn join_remote(dir: &str, name: &str) -> String {
    if dir.is_empty() || dir == "." {
        name.to_string()
    } else if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}
