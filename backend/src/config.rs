//! Application configuration loaded from the environment.
//!
//! Secret keys (`WEBSSH_MASTER_KEY`, `WEBSSH_SESSION_KEY`) are auto-generated
//! and written back to a local `.env` file on first run when missing, so a
//! fresh deployment works with zero configuration while still pinning keys
//! across restarts.

use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use base64::{Engine, engine::general_purpose::STANDARD as B64};

/// Parsed runtime configuration.
#[derive(Clone, Debug)]
pub struct Config {
    pub host: String,
    pub port: u16,
    /// rusqlite-style path, e.g. `sqlite://webssh.db?mode=rwc`.
    #[allow(dead_code)]
    pub database_url: String,
    /// Plain filesystem path derived from `database_url`.
    pub db_path: PathBuf,
    pub master_key: Vec<u8>,
    /// Loaded + validated on startup (tower-sessions derives its own signing
    /// key internally); kept for future explicit-cookie-signing use.
    #[allow(dead_code)]
    pub session_key: Vec<u8>,
    pub session_ttl_secs: i64,
    /// Terminal WebSocket idle timeout in seconds. 0 = never disconnect on
    /// idle (the default). When > 0, a terminal with no traffic for this long
    /// is considered dead and closed. A keepalive heartbeat always runs.
    pub terminal_idle_timeout_secs: u64,
}

impl Config {
    /// Load configuration, persisting freshly-generated secrets to `data/.env`.
    pub fn load() -> Result<Self> {
        // The data directory holds the SQLite database and the generated .env
        // (with secret keys). Ensure it exists.
        let data_dir = data_dir();
        std::fs::create_dir_all(&data_dir)
            .with_context(|| format!("create data dir {}", data_dir.display()))?;
        let env_path = data_dir.join(".env");

        // dotenv-style: read .env if present (very small parser, no dep).
        if let Ok(text) = std::fs::read_to_string(&env_path) {
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=') {
                    let k = k.trim();
                    let v = v.trim().trim_matches('"');
                    if std::env::var(k).is_err() {
                        std::env::set_var(k, v);
                    }
                }
            }
        }

        let host = std::env::var("WEBSSH_HOST").unwrap_or_else(|_| "127.0.0.1".into());
        let port: u16 = std::env::var("WEBSSH_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(3000);
        // Default DB lives in the data dir. The default path is resolved
        // relative to data_dir so users don't need to know where it is.
        let database_url = std::env::var("WEBSSH_DATABASE_URL")
            .unwrap_or_else(|_| format!("sqlite://{}/webssh.db?mode=rwc", data_dir.display()));
        let db_path = derive_db_path(&database_url)?;
        let session_ttl_secs: i64 = std::env::var("WEBSSH_SESSION_TTL_SECS")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(60 * 60 * 24 * 7);
        // Terminal idle timeout (seconds). 0 = never disconnect on idle.
        let terminal_idle_timeout_secs: u64 = std::env::var("WEBSSH_TERMINAL_IDLE_TIMEOUT_SECS")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(0);

        // Ensure secret keys exist; generate + persist on first run.
        ensure_secret("WEBSSH_MASTER_KEY", 32, &env_path)?;
        ensure_secret("WEBSSH_SESSION_KEY", 64, &env_path)?;

        let master_key = decode_secret("WEBSSH_MASTER_KEY", 32)?;
        let session_key = decode_secret("WEBSSH_SESSION_KEY", 64)?;

        Ok(Self {
            host,
            port,
            database_url,
            db_path,
            master_key,
            session_key,
            session_ttl_secs,
            terminal_idle_timeout_secs,
        })
    }
}

/// Convert a rusqlite-style `sqlite://path?...` URL into a filesystem path.
fn derive_db_path(url: &str) -> Result<PathBuf> {
    let path = url
        .strip_prefix("sqlite://")
        .or_else(|| url.strip_prefix("sqlite:"))
        .unwrap_or(url);
    let path = path.split('?').next().unwrap_or(path);
    Ok(PathBuf::from(path))
}

/// The data directory holding the SQLite database and generated `.env`.
/// Defaults to `./data` (relative to the current working directory).
fn data_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("WEBSSH_DATA_DIR") {
        return PathBuf::from(custom);
    }
    PathBuf::from("data")
}

/// Make sure an environment variable holds a base64-encoded secret of the given
/// byte length. If it is missing/invalid, generate a fresh one and append it to
/// `data/.env` (at `env_path`) so it persists across restarts.
fn ensure_secret(name: &str, len: usize, env_path: &std::path::Path) -> Result<()> {
    let existing = std::env::var(name).ok();
    let valid = existing
        .as_deref()
        .and_then(|v| B64.decode(v).ok())
        .map(|bytes| bytes.len() == len)
        .unwrap_or(false);
    if valid {
        return Ok(());
    }

    let mut buf = vec![0u8; len];
    use rand::RngCore;
    rand::thread_rng().fill_bytes(&mut buf);
    let b64 = B64.encode(&buf);
    std::env::set_var(name, &b64);

    // Persist to data/.env (create if missing, replace any stale line for this key).
    let mut content = std::fs::read_to_string(env_path).unwrap_or_default();
    if !content.ends_with('\n') && !content.is_empty() {
        content.push('\n');
    }
    let line = format!("{name}={b64}\n");
    // Remove any stale (possibly empty) line for this key.
    let prefix = format!("{name}=");
    content = content
        .lines()
        .filter(|l| !l.trim_start().starts_with(&prefix))
        .collect::<Vec<_>>()
        .join("\n");
    if !content.ends_with('\n') && !content.is_empty() {
        content.push('\n');
    }
    content.push_str(&line);
    std::fs::write(env_path, content)
        .with_context(|| format!("failed to write {}", env_path.display()))?;
    Ok(())
}

fn decode_secret(name: &str, len: usize) -> Result<Vec<u8>> {
    let raw = std::env::var(name).context(format!("{name} not set"))?;
    let bytes = B64
        .decode(raw)
        .map_err(|e| anyhow!("invalid base64 for {name}: {e}"))?;
    if bytes.len() != len {
        return Err(anyhow!("{name} must be {len} bytes, got {}", bytes.len()));
    }
    Ok(bytes)
}
