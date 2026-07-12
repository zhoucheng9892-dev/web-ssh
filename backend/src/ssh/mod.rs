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
///
/// For password auth we try **both** SSH password methods (`password` then
/// `keyboard-interactive`), each on its own fresh connection. A server may
/// support only one, and worse, may silently drop a request for a method it
/// doesn't support instead of returning a clean failure — which looks like a
/// timeout. So a rejection *or* a timeout on the first method is never fatal:
/// we record it and still try the other. Each method dials independently so a
/// half-finished session never poisons the next attempt.
pub async fn connect(host: &str, port: u16, user: &str, secret: &ConnectionSecret) -> Result<client::Handle<ClientHandler>> {
    match secret.auth_type.as_str() {
        "password" => {
            let password = String::from_utf8_lossy(&secret.secret).to_string();
            // Try both SSH password methods on independent fresh connections.
            // A server may support only one: `password` (single round-trip) or
            // `keyboard-interactive` (PAM, multi-prompt). We can't know in
            // advance, and — importantly — a server that doesn't support the
            // requested method may simply *not respond* rather than sending a
            // clean USERAUTH_FAILURE, which surfaces as a timeout. So neither a
            // rejection nor a timeout on the first method should stop us from
            // trying the other; we only fail if both come back negative.
            for method in [Method::Password, Method::KeyboardInteractive] {
                match try_password_auth(host, port, user, &password, method).await {
                    Ok(AuthOutcome::Success(handle)) => return Ok(handle),
                    Ok(AuthOutcome::Rejected) => {
                        tracing::info!(?method, "auth method rejected, trying next");
                    }
                    Err(e) => {
                        // Timeout or transport error on this method — record it
                        // and still try the other method before giving up.
                        tracing::warn!(?method, error = %e, "auth method failed, trying next");
                    }
                }
            }
            bail!("authentication rejected by server (tried password and keyboard-interactive)");
        }
        "key" => {
            let pem = String::from_utf8_lossy(&secret.secret).to_string();
            let key = russh_keys::decode_secret_key(&pem, None)
                .context("failed to parse private key PEM")?;
            let key_with_alg = PrivateKeyWithHashAlg::new(Arc::new(key), None)
                .map_err(|e| anyhow!("key/hash alg: {e}"))?;
            // Publickey is a single clean attempt.
            let mut handle = dial(host, port).await?;
            let authed = auth_op("publickey", handle.authenticate_publickey(user, key_with_alg)).await?;
            if !authed {
                bail!("authentication rejected by server");
            }
            Ok(handle)
        }
        other => bail!("unsupported auth_type: {other}"),
    }
}

/// Which password auth method to attempt on a fresh connection.
#[derive(Debug, Clone, Copy)]
enum Method {
    Password,
    KeyboardInteractive,
}

/// Outcome of a single password-auth attempt on a fresh connection.
enum AuthOutcome {
    /// Authenticated; the caller owns the live handle.
    Success(client::Handle<ClientHandler>),
    /// Server rejected this method/credentials (try another method or give up).
    Rejected,
}

/// Open a brand-new SSH connection (TCP + handshake) and authenticate with the
/// chosen password method. On rejection the half-open handle is dropped so the
/// session ends cleanly; the caller may then dial again for a fallback method.
async fn try_password_auth(
    host: &str,
    port: u16,
    user: &str,
    password: &str,
    method: Method,
) -> Result<AuthOutcome> {
    let mut handle = dial(host, port).await?;
    let ok = match method {
        Method::Password => {
            tracing::debug!("trying SSH password auth");
            auth_op("password", handle.authenticate_password(user, password.to_string())).await?
        }
        Method::KeyboardInteractive => {
            tracing::debug!("trying SSH keyboard-interactive auth");
            auth_keyboard_interactive(&mut handle, user, password).await?
        }
    };
    if ok {
        Ok(AuthOutcome::Success(handle))
    } else {
        Ok(AuthOutcome::Rejected)
    }
}

/// Establish a fresh TCP + SSH handshake. Factored out so each auth attempt
/// starts from a clean session state (see [`connect`] for why this matters).
async fn dial(host: &str, port: u16) -> Result<client::Handle<ClientHandler>> {
    let config = Arc::new(client::Config {
        // The default `maximum_packet_size` (32 KiB) caps how much payload a
        // single SSH data packet can carry. russh's channel writer sends at
        // most one such packet per round-trip before awaiting the server's
        // window adjust, so with the default this degenerates into
        // stop-and-wait: 32 KiB / RTT ≈ 160 KB/s on a 200 ms LAN link, which
        // matches the ~150 KB/s observed for large SFTP uploads. Raising it to
        // 1 MiB lets each round-trip carry 32× more data, bringing throughput
        // back to the bandwidth limit. SSH only mandates a 32 KiB minimum
        // (RFC 4253 §6.1) and OpenSSH's per-channel limit is well above this.
        maximum_packet_size: 1024 * 1024,
        // Keep enough window credit in flight to keep the pipeline full: ~4
        // maximum-sized packets outstanding before we must wait for an adjust.
        window_size: 4 * 1024 * 1024,
        ..Default::default()
    });
    let handler = ClientHandler;
    tokio::time::timeout(HANDSHAKE_TIMEOUT, client::connect(config, (host, port), handler))
        .await
        .map_err(|_| anyhow!("SSH 握手超时（{}/{host}:{port}）", humandur(HANDSHAKE_TIMEOUT)))?
        .map_err(|e| anyhow!("ssh connect to {host}:{port} failed: {e}"))
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
/// This is the primary password path (tried before the `password` method) and
/// matches what the `ssh` CLI does by default: respond to each PAM prompt with
/// the same password. Covers the typical single-prompt "Password: " case (most
/// Linux PAM setups) and tolerates multi-round / empty-prompt flows. Each round
/// is bounded by [`AUTH_OP_TIMEOUT`] and the total number of rounds by
/// [`MAX_KBDI_ROUNDS`].
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
    // Standard terminal modes that OpenSSH's ssh client sets on every PTY.
    // Previously we only set ECHO=1, which left OPOST/ONLCR unset: the shell's
    // output processing was disabled, so `\n` was not translated to `\r\n`.
    // When readline redraws a wrapped line (e.g. after pressing an arrow key
    // with a long pasted line in the buffer), its control sequences assumed
    // standard output processing but the PTY wasn't doing it — the cursor
    // position and line wrapping desynced, garbling the display.
    let modes = [
        (russh::Pty::VINTR, 3),
        (russh::Pty::VQUIT, 28),
        (russh::Pty::VERASE, 127),
        (russh::Pty::VKILL, 21),
        (russh::Pty::VEOF, 4),
        (russh::Pty::VEOL, 0),
        (russh::Pty::VEOL2, 0),
        (russh::Pty::VSTART, 17),
        (russh::Pty::VSTOP, 19),
        (russh::Pty::VSUSP, 26),
        (russh::Pty::VREPRINT, 18),
        (russh::Pty::VWERASE, 23),
        (russh::Pty::VLNEXT, 22),
        (russh::Pty::VDISCARD, 15),
        (russh::Pty::IUTF8, 1),
        (russh::Pty::ISIG, 1),
        (russh::Pty::ICANON, 1),
        (russh::Pty::ECHO, 1),
        (russh::Pty::ECHOE, 1),
        (russh::Pty::ECHOK, 1),
        (russh::Pty::ECHONL, 0),
        (russh::Pty::NOFLSH, 0),
        (russh::Pty::IEXTEN, 1),
        (russh::Pty::ECHOCTL, 1),
        (russh::Pty::ECHOKE, 1),
        (russh::Pty::PENDIN, 0),
        (russh::Pty::OPOST, 1),
        (russh::Pty::ONLCR, 1),
    ];
    channel
        .request_pty(false, "xterm-256color", cols, rows, 0, 0, &modes)
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
