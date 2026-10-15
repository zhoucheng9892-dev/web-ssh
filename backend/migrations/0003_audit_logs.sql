-- Audit log: every command a user types in a terminal session.
-- No foreign keys (ON DELETE CASCADE) — audit records must survive user /
-- connection deletion for forensic purposes. username and conn_name are
-- snapshots captured at execution time.
CREATE TABLE IF NOT EXISTS audit_logs (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id       INTEGER NOT NULL,
    username      TEXT NOT NULL,
    connection_id INTEGER NOT NULL,
    conn_name     TEXT,
    command       TEXT NOT NULL,
    created_at    TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_audit_user_time ON audit_logs(user_id, created_at);
CREATE INDEX IF NOT EXISTS idx_audit_time ON audit_logs(created_at);
