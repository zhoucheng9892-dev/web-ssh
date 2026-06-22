-- Add account-status flag to users. is_disabled = 1 means the account is
-- frozen: login is refused and active sessions should be invalidated.
ALTER TABLE users ADD COLUMN is_disabled INTEGER NOT NULL DEFAULT 0;
