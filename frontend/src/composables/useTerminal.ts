import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { WebLinksAddon } from '@xterm/addon-web-links'
import { WebglAddon } from '@xterm/addon-webgl'
import { CONTEXT_PATH } from '@/context'

/**
 * A terminal session keeps its WebSocket and xterm instance alive for the whole
 * lifetime of the session — across page switches and tab changes. The xterm is
 * created once inside a persistent "host" <div>; to show it in a pane we simply
 * move that host div into the pane (DOM reparenting preserves the renderer and
 * its scrollback, so history is never lost).
 *
 * Output received while the host is detached (no parent) is buffered in
 * `pending` and flushed on the next attach.
 */
export interface TerminalSession {
  id: string
  connectionId: number
  title: string
  /** Persistent xterm instance; created once, disposed only on close. */
  terminal: Terminal
  fit: FitAddon
  /** Persistent host div that owns the xterm DOM. Moved between panes. */
  host: HTMLDivElement
  /** The pane the host is currently mounted in, or null when detached. */
  pane: HTMLDivElement | null
  ws: WebSocket | null
  closed: boolean
  /** True once the WebSocket handshake succeeded (we received any frame). Used
   *  to tell a rejected handshake apart from a mid-session drop on close. */
  everOpened: boolean
  /** Output buffered while detached (no pane). */
  pending: Uint8Array[]
  cols: number
  rows: number
}

let counter = 0

function terminalUrl(connectionId: number, cols: number, rows: number): string {
  const proto = location.protocol === 'https:' ? 'wss:' : 'ws:'
  // Insert the context path between host and the api path so WebSocket
  // connects under the same sub-path as the app.
  const apiPath = CONTEXT_PATH === '' ? '/api/terminal/connect' : `${CONTEXT_PATH}/api/terminal/connect`
  return (
    `${proto}//${location.host}${apiPath}` +
    `?connection_id=${connectionId}&cols=${cols}&rows=${rows}`
  )
}

function defaultTheme() {
  return {
    background: '#0d1117',
    foreground: '#c9d1d9',
    cursor: '#c9d1d9',
    selectionBackground: '#264f78',
    black: '#0d1117',
    red: '#ff7b72',
    green: '#3fb950',
    yellow: '#d29922',
    blue: '#58a6ff',
    magenta: '#bc8cff',
    cyan: '#39c5cf',
    white: '#c9d1d9',
    brightBlack: '#6e7681',
    brightWhite: '#f0f6fc',
  }
}

/** Create the xterm + its persistent host div, wired to the websocket. */
function createTerminal(cols: number, rows: number): { terminal: Terminal; fit: FitAddon; host: HTMLDivElement } {
  const terminal = new Terminal({
    fontFamily: 'Menlo, Consolas, "DejaVu Sans Mono", monospace',
    fontSize: 13,
    cursorBlink: true,
    cols,
    rows,
    theme: defaultTheme(),
  })
  const fit = new FitAddon()
  terminal.loadAddon(fit)
  terminal.loadAddon(new WebLinksAddon())
  try {
    terminal.loadAddon(new WebglAddon())
  } catch {
    // WebGL not available; fall back to canvas renderer.
  }
  // The host div persists for the session's lifetime; xterm renders into it.
  const host = document.createElement('div')
  host.className = 'xterm-host'
  host.style.width = '100%'
  host.style.height = '100%'
  terminal.open(host)
  return { terminal, fit, host }
}

/** Open a session: persistent xterm + WebSocket. Not attached to any pane yet. */
export function openSession(connectionId: number, title: string): TerminalSession {
  const id = `s${++counter}`
  const { terminal, fit, host } = createTerminal(80, 24)
  const session: TerminalSession = {
    id,
    connectionId,
    title,
    terminal,
    fit,
    host,
    pane: null,
    ws: null,
    closed: false,
    everOpened: false,
    pending: [],
    cols: 80,
    rows: 24,
  }

  const ws = new WebSocket(terminalUrl(connectionId, session.cols, session.rows))
  ws.binaryType = 'arraybuffer'
  session.ws = ws

  // THE critical fix: the PTY was opened at 80×24 (from the WS URL), but the
  // real terminal size is measured by fit() when the pane is attached — which
  // usually happens *before* the WebSocket finishes its handshake. Without
  // this, onResize fires while ws is still CONNECTING, the resize is silently
  // dropped, and the PTY stays at 80×24 forever. The shell's readline then
  // wraps text at 80 columns while xterm.js renders at the real width. Pasting
  // a long line and pressing an arrow key makes readline repaint with the wrong
  // column count → text duplicated, cursor in the wrong place.
  ws.onopen = () => {
    if (session.cols && session.rows) {
      ws.send(JSON.stringify({ type: 'resize', cols: session.cols, rows: session.rows }))
    }
  }

  ws.onmessage = (ev) => {
    session.everOpened = true
    if (typeof ev.data === 'string') {
      try {
        const msg = JSON.parse(ev.data)
        if (msg.type === 'exit') {
          writeOutput(session, `\r\n\x1b[33m[process exited with code ${msg.code}]\x1b[0m`)
        } else if (msg.type === 'ping') {
          // Backend keepalive probe: reply so the server knows we're here and
          // refreshes its idle timer.
          if (ws.readyState === WebSocket.OPEN) {
            ws.send('{"type":"pong"}')
          }
        } else if (msg.type === 'closed') {
          // Server-initiated close (e.g. idle timeout); surface a reason.
          const reason = msg.reason ? `（${msg.reason}）` : ''
          writeOutput(session, `\r\n\x1b[31m[connection closed${reason}]\x1b[0m`)
        }
      } catch {
        /* ignore */
      }
      return
    }
    writeOutput(session, new Uint8Array(ev.data as ArrayBuffer))
  }

  // The browser fires onerror before onclose on a rejected handshake but gives
  // no payload — the HTTP error body is unreachable. onclose distinguishes the
  // two cases: never received a frame => handshake rejected (the probe should
  // have caught it, but races are possible); otherwise a mid-session drop.
  ws.onerror = () => {
    /* details surface in onclose via everOpened/code below */
  }

  ws.onclose = (ev) => {
    session.closed = true
    const reason = ev.reason?.trim()
    if (!session.everOpened && ev.code === 1006) {
      writeOutput(
        session,
        '\r\n\x1b[31m[连接被拒绝：握手失败，请检查目标机器的 sshd 是否可达，或查看后端日志]\x1b[0m',
      )
    } else if (reason) {
      writeOutput(session, `\r\n\x1b[31m[connection closed（${reason}）]\x1b[0m`)
    } else {
      writeOutput(session, '\r\n\x1b[31m[connection closed]\x1b[0m')
    }
  }

  // All outbound data goes through one serial queue so keystrokes and paste
  // data never interleave on the WebSocket.
  let sendChain: Promise<void> = Promise.resolve()
  const encoder = new TextEncoder()

  terminal.onData((data) => {
    if (ws.readyState !== WebSocket.OPEN) return
    sendChain = sendChain
      .then(() => {
        if (ws.readyState === WebSocket.OPEN) {
          ws.send(encoder.encode(data))
        }
      })
      .catch(() => {})
  })
  terminal.onResize(({ cols, rows }) => {
    session.cols = cols
    session.rows = rows
    if (ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify({ type: 'resize', cols, rows }))
    }
  })

  return session
}

/** Write to the xterm. Since the terminal instance is never destroyed while a
 *  session is active, we always write directly; `pending` only matters before
 *  the very first attach (the xterm exists but may not yet be sized). */
function writeOutput(session: TerminalSession, data: Uint8Array | string) {
  if (session.pane) {
    if (typeof data === 'string') session.terminal.writeln(data)
    else session.terminal.write(data)
  } else {
    // Buffer while detached; flushed on attach so nothing is lost.
    session.pending.push(typeof data === 'string' ? new TextEncoder().encode(data) : data)
  }
}

/** Move the session's persistent host div into a pane and flush any buffer. */
export function attachSession(session: TerminalSession, pane: HTMLDivElement) {
  if (session.pane === pane) {
    refit(session)
    return;
  }
  pane.appendChild(session.host)
  session.pane = pane
  if (session.pending.length) {
    for (const chunk of session.pending) session.terminal.write(chunk)
    session.pending = []
  }
  refit(session)
}

/** Detach the host from its pane (keep xterm + scrollback alive, unmounted). */
export function detachSession(session: TerminalSession) {
  if (session.host.parentElement) {
    session.host.parentElement.removeChild(session.host)
  }
  session.pane = null
}

/** Re-fit after the container resizes (call on window resize / tab switch).
 *  Debounced per session: rapid tab switches / window drags fire multiple fit()
 *  calls that each trigger a PTY resize round-trip; without debouncing the
 *  remote can process a stale size, desyncing the geometry. */
const refitTimers = new WeakMap<TerminalSession, ReturnType<typeof setTimeout>>()
export function refit(session: TerminalSession) {
  if (!session.pane) return
  const existing = refitTimers.get(session)
  if (existing) clearTimeout(existing)
  const timer = setTimeout(() => {
    try {
      session.fit.fit()
    } catch {
      /* terminal not yet open */
    }
  }, 100)
  refitTimers.set(session, timer)
}

/** Fully close a session: tear down the websocket and dispose the terminal. */
export function closeSession(session: TerminalSession) {
  try {
    session.ws?.close()
  } catch {
    /* ignore */
  }
  if (session.host.parentElement) {
    session.host.parentElement.removeChild(session.host)
  }
  session.terminal.dispose()
  session.pane = null
  session.closed = true
}
