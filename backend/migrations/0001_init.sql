-- Web-SSH schema init
-- Users: web-ssh application accounts (separate from SSH login credentials)
CREATE TABLE IF NOT EXISTS users (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    username      TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    is_admin      INTEGER NOT NULL DEFAULT 0,
    created_at    TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Connections: persisted SSH connection profiles, scoped per user.
-- The SSH credential (password or private key PEM) is encrypted at rest
-- with AES-256-GCM before being stored in encrypted_secret.
CREATE TABLE IF NOT EXISTS connections (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id          INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name             TEXT NOT NULL,
    host             TEXT NOT NULL,
    port             INTEGER NOT NULL DEFAULT 22,
    username         TEXT NOT NULL,
    auth_type        TEXT NOT NULL,         -- 'password' | 'key'
    encrypted_secret BLOB NOT NULL,         -- AES-GCM ciphertext
    iv               BLOB NOT NULL,         -- 12-byte nonce
    last_used_at     TEXT,
    created_at       TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_connections_user ON connections(user_id);
