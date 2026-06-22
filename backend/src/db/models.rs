//! Row models and per-table query helpers.
//!
//! All queries are scoped to the connection pool in [`super`] via
//! [`super::with_conn`]. Each query that touches `connections` filters by
//! `user_id` to enforce per-user data isolation.

use deadpool_sqlite::Pool;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

use super::with_conn;
use anyhow::Result;

/// A persisted SSH connection profile (credentials never serialized to clients).
#[derive(Debug, Clone, Serialize)]
pub struct Connection {
    pub id: i64,
    pub user_id: i64,
    pub name: String,
    pub host: String,
    pub port: i16,
    pub username: String,
    pub auth_type: String,
    pub last_used_at: Option<String>,
    pub created_at: String,
}

/// Plaintext secret used only inside the server to dial SSH.
pub struct ConnectionSecret {
    pub auth_type: String, // "password" | "key"
    pub secret: Vec<u8>, // password bytes or PEM key bytes
}

#[derive(Debug, Clone)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
    pub is_admin: bool,
    pub is_disabled: bool,
}

/// Public user info (no password hash) for admin listing and the "me" view.
#[derive(Debug, Clone, Serialize)]
pub struct UserInfo {
    pub id: i64,
    pub username: String,
    pub is_admin: bool,
    pub is_disabled: bool,
    pub created_at: String,
}

impl From<&User> for UserInfo {
    fn from(u: &User) -> Self {
        Self {
            id: u.id,
            username: u.username.clone(),
            is_admin: u.is_admin,
            is_disabled: u.is_disabled,
            created_at: String::new(),
        }
    }
}

/// Payload for creating/updating a connection. `secret` is plaintext from the
/// browser and encrypted before storage; optional on update (blank = keep).
#[derive(Debug, Clone, Deserialize)]
pub struct ConnectionInput {
    pub name: String,
    pub host: String,
    pub port: Option<i16>,
    pub username: String,
    pub auth_type: String, // "password" | "key"
    pub secret: Option<String>, // password or PEM private key; optional on update
}

/// Snapshot of insert/update fields captured into a closure.
#[derive(Debug, Clone)]
pub(super) struct ConnFields {
    pub name: String,
    pub host: String,
    pub port: i16,
    pub username: String,
    pub auth_type: String,
}

impl From<&ConnectionInput> for ConnFields {
    fn from(i: &ConnectionInput) -> Self {
        Self {
            name: i.name.clone(),
            host: i.host.clone(),
            port: i.port.unwrap_or(22),
            username: i.username.clone(),
            auth_type: i.auth_type.clone(),
        }
    }
}

/// Lightweight connection profile fields needed to dial SSH (no secret).
#[derive(Debug, Clone)]
pub struct ConnectionDial {
    pub host: String,
    pub port: i16,
    pub username: String,
}

// --- users ------------------------------------------------------------------

pub async fn count_users(pool: &Pool) -> Result<i64> {
    with_conn(pool, |c| c.query_row("SELECT COUNT(*) FROM users", [], |r| r.get::<_, i64>(0)))
        .await
}

pub async fn create_user(
    pool: &Pool,
    username: &str,
    password_hash: &str,
    is_admin: bool,
) -> Result<i64> {
    let username = username.to_string();
    let password_hash = password_hash.to_string();
    with_conn(pool, move |c| {
        c.query_row(
            "INSERT INTO users(username, password_hash, is_admin) VALUES (?1,?2,?3) RETURNING id",
            rusqlite::params![&username, &password_hash, is_admin as i64],
            |r| r.get::<_, i64>(0),
        )
    })
    .await
}

pub async fn find_user_by_name(pool: &Pool, username: &str) -> Result<Option<User>> {
    let username = username.to_string();
    with_conn(pool, move |c| {
        c.query_row(
            "SELECT id, username, password_hash, is_admin, is_disabled FROM users WHERE username=?1",
            rusqlite::params![&username],
            row_to_user,
        )
        .optional()
    })
    .await
}

pub async fn get_user(pool: &Pool, id: i64) -> Result<Option<User>> {
    with_conn(pool, move |c| {
        c.query_row(
            "SELECT id, username, password_hash, is_admin, is_disabled FROM users WHERE id=?1",
            rusqlite::params![id],
            row_to_user,
        )
        .optional()
    })
    .await
}

fn row_to_user(r: &rusqlite::Row) -> rusqlite::Result<User> {
    Ok(User {
        id: r.get(0)?,
        username: r.get(1)?,
        password_hash: r.get(2)?,
        is_admin: r.get::<_, i64>(3)? != 0,
        is_disabled: r.get::<_, i64>(4)? != 0,
    })
}

/// List all users with their created_at (for the admin management page).
pub async fn list_users(pool: &Pool) -> Result<Vec<UserInfo>> {
    with_conn(pool, move |c| {
        let mut stmt = c.prepare(
            "SELECT id, username, is_admin, is_disabled, created_at FROM users ORDER BY id",
        )?;
        let v = stmt
            .query_map([], |r| {
                Ok(UserInfo {
                    id: r.get(0)?,
                    username: r.get(1)?,
                    is_admin: r.get::<_, i64>(2)? != 0,
                    is_disabled: r.get::<_, i64>(3)? != 0,
                    created_at: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    })
    .await
}

/// Set a user's disabled flag (freeze / unfreeze). Returns true if updated.
pub async fn set_user_disabled(pool: &Pool, id: i64, disabled: bool) -> Result<bool> {
    with_conn(pool, move |c| {
        Ok(c.execute(
            "UPDATE users SET is_disabled=?1 WHERE id=?2",
            rusqlite::params![disabled as i64, id],
        )? > 0)
    })
    .await
}

/// Replace a user's password hash (admin reset / self change).
pub async fn update_user_password(pool: &Pool, id: i64, password_hash: &str) -> Result<bool> {
    let password_hash = password_hash.to_string();
    with_conn(pool, move |c| {
        Ok(c.execute(
            "UPDATE users SET password_hash=?1 WHERE id=?2",
            rusqlite::params![&password_hash, id],
        )? > 0)
    })
    .await
}

/// Promote/demote a user's admin flag.
pub async fn set_user_admin(pool: &Pool, id: i64, is_admin: bool) -> Result<bool> {
    with_conn(pool, move |c| {
        Ok(c.execute(
            "UPDATE users SET is_admin=?1 WHERE id=?2",
            rusqlite::params![is_admin as i64, id],
        )? > 0)
    })
    .await
}

/// Permanently delete a user.
pub async fn delete_user(pool: &Pool, id: i64) -> Result<bool> {
    with_conn(pool, move |c| {
        Ok(c.execute("DELETE FROM users WHERE id=?1", rusqlite::params![id])? > 0)
    })
    .await
}

// --- connections ------------------------------------------------------------

pub async fn list_connections(pool: &Pool, user_id: i64) -> Result<Vec<Connection>> {
    with_conn(pool, move |c| {
        let mut stmt = c.prepare(
            "SELECT id, user_id, name, host, port, username, auth_type, last_used_at, created_at
             FROM connections WHERE user_id=?1 ORDER BY name",
        )?;
        let v = stmt
            .query_map(rusqlite::params![user_id], |r| {
                Ok(Connection {
                    id: r.get(0)?,
                    user_id: r.get(1)?,
                    name: r.get(2)?,
                    host: r.get(3)?,
                    port: r.get(4)?,
                    username: r.get(5)?,
                    auth_type: r.get(6)?,
                    last_used_at: r.get(7)?,
                    created_at: r.get(8)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    })
    .await
}

pub async fn insert_connection(
    pool: &Pool,
    user_id: i64,
    input: &ConnectionInput,
    encrypted: &[u8],
    iv: &[u8],
) -> Result<i64> {
    let f = ConnFields::from(input);
    let enc = encrypted.to_vec();
    let iv = iv.to_vec();
    with_conn(pool, move |c| {
        c.query_row(
            "INSERT INTO connections(user_id, name, host, port, username, auth_type, encrypted_secret, iv)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8) RETURNING id",
            rusqlite::params![user_id, f.name, f.host, f.port, f.username, f.auth_type, enc, iv],
            |r| r.get::<_, i64>(0),
        )
    })
    .await
}

pub async fn update_connection(
    pool: &Pool,
    user_id: i64,
    id: i64,
    input: &ConnectionInput,
    encrypted: Option<(&[u8], &[u8])>,
) -> Result<bool> {
    let f = ConnFields::from(input);
    let enc = encrypted.map(|(c, n)| (c.to_vec(), n.to_vec()));
    with_conn(pool, move |c| {
        let n = if let Some((enc, iv)) = enc {
            c.execute(
                "UPDATE connections SET name=?1, host=?2, port=?3, username=?4, auth_type=?5,
                 encrypted_secret=?6, iv=?7 WHERE id=?8 AND user_id=?9",
                rusqlite::params![f.name, f.host, f.port, f.username, f.auth_type, enc, iv, id, user_id],
            )?
        } else {
            c.execute(
                "UPDATE connections SET name=?1, host=?2, port=?3, username=?4, auth_type=?5
                 WHERE id=?6 AND user_id=?7",
                rusqlite::params![f.name, f.host, f.port, f.username, f.auth_type, id, user_id],
            )?
        };
        Ok(n > 0)
    })
    .await
}

pub async fn delete_connection(pool: &Pool, user_id: i64, id: i64) -> Result<bool> {
    with_conn(pool, move |c| {
        Ok(c.execute(
            "DELETE FROM connections WHERE id=?1 AND user_id=?2",
            rusqlite::params![id, user_id],
        )? > 0)
    })
    .await
}

pub async fn get_connection_secret(
    pool: &Pool,
    user_id: i64,
    id: i64,
) -> Result<Option<(String, Vec<u8>, Vec<u8>)>> {
    with_conn(pool, move |c| {
        c.query_row(
            "SELECT auth_type, encrypted_secret, iv FROM connections WHERE id=?1 AND user_id=?2",
            rusqlite::params![id, user_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Vec<u8>>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                ))
            },
        )
        .optional()
    })
    .await
}

pub async fn touch_last_used(pool: &Pool, id: i64) -> Result<()> {
    with_conn(pool, move |c| {
        c.execute(
            "UPDATE connections SET last_used_at=datetime('now') WHERE id=?1",
            rusqlite::params![id],
        )?;
        Ok(())
    })
    .await
}

pub async fn get_connection_dial(pool: &Pool, user_id: i64, id: i64) -> Result<Option<ConnectionDial>> {
    with_conn(pool, move |c| {
        c.query_row(
            "SELECT host, port, username FROM connections WHERE id=?1 AND user_id=?2",
            rusqlite::params![id, user_id],
            |r| {
                Ok(ConnectionDial {
                    host: r.get(0)?,
                    port: r.get(1)?,
                    username: r.get(2)?,
                })
            },
        )
        .optional()
    })
    .await
}
