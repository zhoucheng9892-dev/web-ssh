//! Shared application state and the SFTP session cache type.

use std::{collections::HashMap, sync::Arc};

use deadpool_sqlite::Pool;

use crate::config::Config;

/// Shared application state available to every handler via the `State` extractor.
#[derive(Clone)]
pub struct AppState {
    pub pool: Pool,
    pub config: Arc<Config>,
    /// SFTP session cache keyed by (user_id, connection_id).
    pub sftp: Arc<tokio::sync::RwLock<HashMap<(i64, i64), CachedSftp>>>,
}

/// A cached SFTP session: `Arc<Mutex<Option<SftpSession>>>`. The `Mutex`
/// serialises operations on the single subsystem channel; SFTP is
/// request/response so that's fine.
pub type CachedSftp = Arc<tokio::sync::Mutex<Option<russh_sftp::client::SftpSession>>>;
