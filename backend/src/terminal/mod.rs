//! WebSocket ↔ SSH PTY bridge.
//!
//! Protocol:
//! - binary frames carry raw terminal bytes, bidirectionally;
//! - text frames carry JSON control messages: `{"type":"resize","cols":..,"rows":..}`
//!   and `{"type":"ping"}`.

use axum::{
    extract::{Query, State, WebSocketUpgrade},
    response::Response,
};
use russh::{ChannelMsg, client};
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::Instrument;

use crate::{
    auth::extractors::{AuthUser, resolve_secret},
    db::models::{get_connection_dial, touch_last_used},
    error::{AppError, AppResult},
    ssh,
    state::AppState,
};

#[derive(Deserialize)]
pub struct ConnectQuery {
    pub connection_id: i64,
    #[serde(default = "default_cols")]
    pub cols: u32,
    #[serde(default = "default_rows")]
    pub rows: u32,
}

fn default_cols() -> u32 {
    80
}
fn default_rows() -> u32 {
    24
}

/// Instructions sent from the websocket reader task to the channel driver.
enum ToChannel {
    Data(Vec<u8>),
    Resize(u32, u32),
    /// Any inbound frame; used to refresh the idle timer.
    Activity,
    Closed,
}

/// `GET /api/terminal/connect` — upgrade to WebSocket and bridge to a shell.
///
/// The handshake is an HTTP GET, so the session cookie is present and
/// [`AuthUser`] resolves normally. SSH connect happens before the upgrade so
/// dial failures surface as clean HTTP errors.
pub async fn connect(
    State(state): State<AppState>,
    user: AuthUser,
    Query(q): Query<ConnectQuery>,
    ws: WebSocketUpgrade,
) -> AppResult<Response> {
    let secret = resolve_secret(&state, user.0, q.connection_id).await?;
    let dial = get_connection_dial(&state.pool, user.0, q.connection_id)
        .await?
        .ok_or(AppError::NotFound)?;

    let handle = ssh::connect(&dial.host, dial.port as u16, &dial.username, &secret)
        .await
        .map_err(|e| AppError::BadRequest(format!("SSH 连接失败: {e}")))?;
    let channel = ssh::open_shell(&handle, q.cols, q.rows)
        .await
        .map_err(|e| AppError::BadRequest(format!("打开 shell 失败: {e}")))?;
    let _ = touch_last_used(&state.pool, q.connection_id).await;

    let idle_timeout = state.config.terminal_idle_timeout_secs;
    Ok(ws.on_upgrade(move |socket| {
        run_bridge(socket, channel, idle_timeout)
            .instrument(tracing::info_span!("terminal", conn = q.connection_id))
    }))
}

/// Drive the SSH channel and the WebSocket concurrently.
///
/// The driver owns `channel`, forwards browser input to it, and streams channel
/// output back to the browser. A heartbeat keeps the connection alive; if
/// `idle_timeout_secs > 0` and no traffic flows for that long, the session is
/// treated as dead and closed.
async fn run_bridge(
    socket: axum::extract::ws::WebSocket,
    mut channel: russh::Channel<client::Msg>,
    idle_timeout_secs: u64,
) {
    use axum::extract::ws::Message;
    use futures::{SinkExt, StreamExt};
    use std::time::{Duration, Instant};

    let (mut ws_sink, mut ws_stream) = socket.split();
    let (to_channel_tx, mut to_channel_rx) = mpsc::channel::<ToChannel>(64);

    // Heartbeat: ping the client every HEARTBEAT_SECS to keep the connection
    // alive (defeats idle-killing NATs/proxies and remote sshd ClientAlive).
    const HEARTBEAT_SECS: u64 = 30;
    let mut heartbeat = tokio::time::interval(Duration::from_secs(HEARTBEAT_SECS));
    // Don't fire immediately on the first tick (we just connected).
    heartbeat.tick().await;

    // Tracks the last time any data flowed in either direction.
    let mut last_activity = Instant::now();
    // Reader: websocket -> mpsc (driver consumes). Any inbound frame counts as
    // activity (refreshes the idle timer); ping/pong frames are activity only.
    let reader = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_stream.next().await {
            let cmd = match msg {
                Message::Binary(bytes) => ToChannel::Data(bytes.to_vec()),
                Message::Text(text) => match serde_json::from_str::<ControlMsg>(&text) {
                    Ok(c) if c.type_ == "resize" => {
                        ToChannel::Resize(c.cols.unwrap_or(80), c.rows.unwrap_or(24))
                    }
                    _ => ToChannel::Activity, // covers our app-level "ping"
                },
                Message::Close(_) => ToChannel::Closed,
                Message::Ping(_) | Message::Pong(_) => ToChannel::Activity,
            };
            let is_closed = matches!(cmd, ToChannel::Closed);
            if to_channel_tx.send(cmd).await.is_err() || is_closed {
                break;
            }
        }
    });

    // Driver: own `channel`, forward input, and emit output to the websocket.
    // Also runs a heartbeat and (optionally) an idle watchdog.
    loop {
        tokio::select! {
            // ssh -> ws
            msg = channel.wait() => match msg {
                Some(ChannelMsg::Data { data }) => {
                    last_activity = Instant::now();
                    if ws_sink.send(Message::Binary(data.to_vec().into())).await.is_err() {
                        break;
                    }
                }
                Some(ChannelMsg::ExtendedData { data, .. }) => {
                    last_activity = Instant::now();
                    if ws_sink.send(Message::Binary(data.to_vec().into())).await.is_err() {
                        break;
                    }
                }
                Some(ChannelMsg::ExitStatus { exit_status }) => {
                    let _ = ws_sink
                        .send(Message::Text(
                            format!(r#"{{"type":"exit","code":{exit_status}}}"#).into(),
                        ))
                        .await;
                    let _ = ws_sink.send(Message::Close(None)).await;
                    break;
                }
                Some(ChannelMsg::Eof) | None => {
                    let _ = ws_sink.send(Message::Close(None)).await;
                    break;
                }
                Some(other) => {
                    tracing::debug!(?other, "channel msg ignored");
                }
            },
            // ws -> ssh
            cmd = to_channel_rx.recv() => match cmd {
                Some(ToChannel::Data(bytes)) => {
                    last_activity = Instant::now();
                    if channel.data(bytes.as_ref()).await.is_err() {
                        break;
                    }
                }
                Some(ToChannel::Resize(cols, rows)) => {
                    last_activity = Instant::now();
                    let _ = channel.window_change(cols, rows, 0, 0).await;
                }
                Some(ToChannel::Activity) => {
                    last_activity = Instant::now();
                }
                Some(ToChannel::Closed) | None => break,
            },
            // Heartbeat: periodically ping the client so the connection stays
            // alive (NAT/proxy/ssh-sshd idle killers) and so we can detect a
            // half-open link when idle timeouts are enabled.
            _ = heartbeat.tick() => {
                let _ = ws_sink
                    .send(Message::Text(r#"{"type":"ping"}"#.into()))
                    .await;
            }
            // Idle watchdog: only armed when idle_timeout_secs > 0. Fires when
            // no traffic has flowed for that long; we then close the session.
            _ = idle_future(idle_timeout_secs, last_activity) => {
                tracing::info!(idle_secs = idle_timeout_secs, "terminal idle timeout, closing");
                let _ = ws_sink.send(Message::Text(
                    r#"{"type":"closed","reason":"idle timeout"}"#.into(),
                )).await;
                let _ = ws_sink.send(Message::Close(None)).await;
                break;
            }
        }
    }

    let _ = channel.eof().await;
    let _ = channel.close().await;
    reader.abort();
}

#[derive(Deserialize)]
struct ControlMsg {
    #[serde(rename = "type")]
    type_: String,
    cols: Option<u32>,
    rows: Option<u32>,
}

/// A future that completes at `last_activity + idle_secs`, or never if
/// `idle_secs == 0` (idle timeout disabled). Recreated each loop iteration so
/// the deadline tracks the latest activity.
async fn idle_future(idle_secs: u64, last_activity: std::time::Instant) {
    if idle_secs == 0 {
        // Disabled: park forever. Selecting on this never fires.
        std::future::pending::<()>().await;
        return;
    }
    let elapsed = last_activity.elapsed();
    let timeout = std::time::Duration::from_secs(idle_secs);
    let remaining = timeout.checked_sub(elapsed).unwrap_or_default();
    tokio::time::sleep(remaining).await;
}
