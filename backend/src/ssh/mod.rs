//! SSH client wrapper built on `russh`.
//!
//! [`SshClient`] holds an authenticated connection. From it we open either a
//! PTY shell channel (for the terminal bridge) or an SFTP subsystem channel
//! (for file management). Server host keys are accepted on a trust-on-first-use
//! basis and their fingerprint logged; this can be tightened to a known_hosts
//! store later.

use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use russh::client;
use russh_keys::key::PrivateKeyWithHashAlg;
use russh_sftp::client::SftpSession;

use crate::db::models::ConnectionSecret;

/// Connect + authenticate and return a handle the caller can open channels on.
pub async fn connect(host: &str, port: u16, user: &str, secret: &ConnectionSecret) -> Result<client::Handle<ClientHandler>> {
    let config = Arc::new(client::Config {
        ..Default::default()
    });
    let addr = (host, port);
    let handler = ClientHandler;
    let mut handle = client::connect(config, addr, handler)
        .await
        .map_err(|e| anyhow!("ssh connect to {host}:{port} failed: {e}"))?;

    let authed = match secret.auth_type.as_str() {
        "password" => {
            let password = String::from_utf8_lossy(&secret.secret).to_string();
            handle
                .authenticate_password(user, password)
                .await
                .map_err(|e| anyhow!("password auth error: {e}"))?
        }
        "key" => {
            let pem = String::from_utf8_lossy(&secret.secret).to_string();
            let key = russh_keys::decode_secret_key(&pem, None)
                .context("failed to parse private key PEM")?;
            let key_with_alg = PrivateKeyWithHashAlg::new(Arc::new(key), None)
                .map_err(|e| anyhow!("key/hash alg: {e}"))?;
            handle
                .authenticate_publickey(user, key_with_alg)
                .await
                .map_err(|e| anyhow!("publickey auth error: {e}"))?
        }
        other => bail!("unsupported auth_type: {other}"),
    };
    if !authed {
        bail!("authentication rejected by server");
    }
    Ok(handle)
}

/// Per-connection event handler. TOFU host-key policy: accept and log the
/// fingerprint. Override `check_server_key` to enforce known_hosts later.
pub struct ClientHandler;

#[async_trait::async_trait]
impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh_keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        let fp = server_public_key.fingerprint(russh_keys::ssh_key::HashAlg::Sha256);
        tracing::info!(fingerprint = %fp, "accepting server host key (TOFU)");
        Ok(true)
    }
}

/// Open a PTY + shell channel and return it for the terminal bridge to drive.
pub async fn open_shell(
    handle: &client::Handle<ClientHandler>,
    cols: u32,
    rows: u32,
) -> Result<russh::Channel<russh::client::Msg>> {
    let channel = handle
        .channel_open_session()
        .await
        .map_err(|e| anyhow!("open session channel: {e}"))?;
    channel
        .request_pty(
            false,
            "xterm-256color",
            cols,
            rows,
            0,
            0,
            &[(russh::Pty::ECHO, 1)],
        )
        .await
        .map_err(|e| anyhow!("request pty: {e}"))?;
    channel
        .request_shell(true)
        .await
        .map_err(|e| anyhow!("request shell: {e}"))?;
    Ok(channel)
}

/// Open an SFTP subsystem channel and initialise the high-level session.
pub async fn open_sftp(handle: &client::Handle<ClientHandler>) -> Result<SftpSession> {
    let channel = handle
        .channel_open_session()
        .await
        .map_err(|e| anyhow!("open sftp channel: {e}"))?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|e| anyhow!("request sftp subsystem: {e}"))?;
    let sftp = SftpSession::new(channel.into_stream())
        .await
        .context("init sftp session")?;
    Ok(sftp)
}
