//! SSH client wrapper built on `russh`.
//!
//! [`SshClient`] holds an authenticated connection. From it we open either a
//! PTY shell channel (for the terminal bridge) or an SFTP subsystem channel
//! (for file management). Server host keys are accepted on a trust-on-first-use
//! basis and their fingerprint logged; this can be tightened to a known_hosts
//! store later.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use russh::client;
use russh_keys::key::PrivateKeyWithHashAlg;
use russh_sftp::client::SftpSession;

use crate::db::models::ConnectionSecret;

/// Overall budget for TCP connect + SSH handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);
/// Per-round timeout for each auth await (password attempt or one kbd-interactive
/// round-trip). Generous enough for PAM/DNS delays, tight enough to fail fast
/// when the server is silently dropping auth requests.
const AUTH_OP_TIMEOUT: Duration = Duration::from_secs(10);
/// Cap on keyboard-interactive rounds. Real PAM password auth takes 1 round;
/// anything past 3 is almost certainly multi-factor (OTP etc.) we can't handle.
const MAX_KBDI_ROUNDS: usize = 3;

/// Connect + authenticate and return a handle the caller can open channels on.
///
/// Every step is wrapped in a timeout so a misbehaving server (silent drops,
/// slow PAM, infinite kbd-interactive prompts) cannot park a connection slot
/// indefinitely and starve other terminals.
pub async fn connect(host: &str, port: u16, user: &str, secret: &ConnectionSecret) -> Result<client::Handle<ClientHandler>> {
    let config = Arc::new(client::Config {
        ..Default::default()
    });
    let addr = (host, port);
    let handler = ClientHandler;
    let mut handle = tokio::time::timeout(HANDSHAKE_TIMEOUT, client::connect(config, addr, handler))
        .await
        .map_err(|_| anyhow!("SSH 握手超时（{}/{host}:{port}）", humandur(HANDSHAKE_TIMEOUT)))?
        .map_err(|e| anyhow!("ssh connect to {host}:{port} failed: {e}"))?;

    let authed = match secret.auth_type.as_str() {
        "password" => {
            let password = String::from_utf8_lossy(&secret.secret).to_string();
            // Try the SSH `password` method first; on rejection fall back to
            // `keyboard-interactive` (PAM). Some sshd — notably older CentOS /
            // Ubuntu — ship with `PasswordAuthentication no` and only allow
            // KbdInteractiveAuthentication, which `ssh` CLI does silently.
            match auth_op("password", handle.authenticate_password(user, password.clone())).await? {
                true => true,
                false => match auth_keyboard_interactive(&mut handle, user, &password).await {
                    Ok(true) => {
                        tracing::info!("auth succeeded via keyboard-interactive fallback");
                        true
                    }
                    Ok(false) => false,
                    Err(e) => return Err(anyhow!("keyboard-interactive auth error: {e}")),
                },
            }
        }
        "key" => {
            let pem = String::from_utf8_lossy(&secret.secret).to_string();
            let key = russh_keys::decode_secret_key(&pem, None)
                .context("failed to parse private key PEM")?;
            let key_with_alg = PrivateKeyWithHashAlg::new(Arc::new(key), None)
                .map_err(|e| anyhow!("key/hash alg: {e}"))?;
            auth_op("publickey", handle.authenticate_publickey(user, key_with_alg)).await?
        }
        other => bail!("unsupported auth_type: {other}"),
    };
    if !authed {
        bail!("authentication rejected by server");
    }
    Ok(handle)
}

/// Wrap an auth future in [`AUTH_OP_TIMEOUT`] and normalise errors so a hung
/// server fails fast with a descriptive message instead of parking forever.
async fn auth_op<F>(label: &str, f: F) -> Result<bool>
where
    F: std::future::Future<Output = Result<bool, russh::Error>>,
{
    match tokio::time::timeout(AUTH_OP_TIMEOUT, f).await {
        Ok(Ok(b)) => Ok(b),
        Ok(Err(e)) => Err(anyhow!("{label} auth error: {e}")),
        Err(_) => Err(anyhow!("{label} auth 超时（{}）", humandur(AUTH_OP_TIMEOUT))),
    }
}

/// Drive keyboard-interactive auth, replying with `password` to every prompt.
///
/// Mirrors what `ssh` CLI does when `password` is rejected: respond to each PAM
/// prompt with the same password. Covers the typical single-prompt "Password: "
/// case (most Linux PAM setups) and tolerates multi-round / empty-prompt flows.
/// Each round is bounded by [`AUTH_OP_TIMEOUT`] and the total number of rounds
/// by [`MAX_KBDI_ROUNDS`].
async fn auth_keyboard_interactive(
    handle: &mut client::Handle<ClientHandler>,
    user: &str,
    password: &str,
) -> Result<bool> {
    use russh::client::KeyboardInteractiveAuthResponse;

    let mut resp = tokio::time::timeout(
        AUTH_OP_TIMEOUT,
        handle.authenticate_keyboard_interactive_start(user, None::<String>),
    )
    .await
    .map_err(|_| anyhow!("kbd-interactive 起始超时（{}）", humandur(AUTH_OP_TIMEOUT)))?
    .map_err(|e| anyhow!("kbd-interactive start: {e}"))?;

    for round in 1..=MAX_KBDI_ROUNDS {
        match resp {
            KeyboardInteractiveAuthResponse::Success => return Ok(true),
            KeyboardInteractiveAuthResponse::Failure => return Ok(false),
            KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } => {
                tracing::debug!(
                    round,
                    prompt_count = prompts.len(),
                    "kbd-interactive challenge, replying with password"
                );
                let responses = if prompts.is_empty() {
                    Vec::new()
                } else {
                    std::iter::repeat_with(|| password.to_string())
                        .take(prompts.len())
                        .collect()
                };
                resp = tokio::time::timeout(
                    AUTH_OP_TIMEOUT,
                    handle.authenticate_keyboard_interactive_respond(responses),
                )
                .await
                .map_err(|_| anyhow!("kbd-interactive 第 {round} 轮超时（{}）", humandur(AUTH_OP_TIMEOUT)))?
                .map_err(|e| anyhow!("kbd-interactive respond: {e}"))?;
            }
        }
    }
    bail!("kbd-interactive 超过 {MAX_KBDI_ROUNDS} 轮（可能是我们不支持的多因素认证）");
}

/// Compact human-readable duration, e.g. `15s` / `1m30s`.
fn humandur(d: Duration) -> String {
    let secs = d.as_secs();
    if secs >= 60 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    }
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
