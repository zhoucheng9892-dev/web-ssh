//! Database access layer.
//!
//! This module owns the connection pool, schema migrations, and the shared
//! `with_conn` helper. Row models and per-table queries live in
//! [`models`](crate::db::models).

use std::path::Path;

use anyhow::{Context, Result};
use deadpool_sqlite::{Config as PoolConfig, Pool, Runtime};

pub mod models;

/// Build a `deadpool-sqlite` pool over the given database file, apply pragma
/// tuning, and run schema migrations.
pub fn create_pool(path: &Path) -> Result<Pool> {
    let cfg = PoolConfig::new(path);
    let pool = cfg
        .create_pool(Runtime::Tokio1)
        .context("failed to create sqlite pool")?;

    let pool2 = pool.clone();
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async move {
            let conn = pool2.get().await?;
            conn.interact(|c| {
                c.pragma_update(None, "journal_mode", "WAL")?;
                c.pragma_update(None, "foreign_keys", "ON")?;
                c.pragma_update(None, "busy_timeout", 5000)?;
                Ok::<_, rusqlite::Error>(())
            })
            .await
            .map_err(|e| anyhow::anyhow!("db init task: {e}"))??;
            Ok::<_, anyhow::Error>(())
        })
    })?;

    run_migrations(&pool)?;
    Ok(pool)
}

/// Apply all bundled migration SQL files, tracking applied ones in a
/// `_migrations` table so each runs exactly once (safe across restarts).
fn run_migrations(pool: &Pool) -> Result<()> {
    // (id, sql) pairs, in order.
    let migrations: &[(&str, &str)] = &[
        ("0001_init", include_str!("../../migrations/0001_init.sql")),
        ("0002_users_disabled", include_str!("../../migrations/0002_users_disabled.sql")),
    ];
    let pool = pool.clone();
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async move {
            let conn = pool.get().await?;
            conn.interact(move |c| {
                c.execute_batch(
                    "CREATE TABLE IF NOT EXISTS _migrations(id TEXT PRIMARY KEY);",
                )?;
                for (id, sql) in migrations {
                    let done: bool = c
                        .query_row(
                            "SELECT EXISTS(SELECT 1 FROM _migrations WHERE id=?1)",
                            rusqlite::params![id],
                            |r| r.get::<_, i64>(0),
                        )
                        .map(|n| n != 0)?;
                    if done {
                        continue;
                    }
                    c.execute_batch(sql)?;
                    c.execute(
                        "INSERT INTO _migrations(id) VALUES (?1)",
                        rusqlite::params![id],
                    )?;
                }
                Ok::<_, rusqlite::Error>(())
            })
            .await
            .map_err(|e| anyhow::anyhow!("migration task: {e}"))??;
            Ok::<_, anyhow::Error>(())
        })
    })
}

/// Run a closure on a pooled connection, mapping both rusqlite and join errors.
///
/// All per-table query helpers in [`models`] route through this so error
/// handling stays uniform.
pub async fn with_conn<T, F>(pool: &Pool, f: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(&mut rusqlite::Connection) -> rusqlite::Result<T> + Send + 'static,
{
    let conn = pool.get().await?;
    conn.interact(f)
        .await
        .map_err(|e| anyhow::anyhow!("db task join: {e}"))?
        .map_err(Into::into)
}
