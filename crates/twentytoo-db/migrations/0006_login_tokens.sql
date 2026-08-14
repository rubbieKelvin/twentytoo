-- Short-lived, single-use step tokens for the two-step login flow. The
-- client holds a random token; the table stores only its SHA-256 hex hash,
-- matching the sessions-table pattern (a leaked table yields no usable
-- credentials). `purpose` records which step the token proves: `email_ok`
-- (email step confirmed) or `code_ok` (code step confirmed). `code_hash`
-- is the SHA-256 hex of the emailed code when email confirmation is on;
-- `attempts` bounds guessing (5 tries, then the token is consumed).
CREATE TABLE login_tokens (
    token_hash text PRIMARY KEY,
    email      text NOT NULL,
    user_id    uuid REFERENCES users (id) ON DELETE CASCADE,
    purpose    text NOT NULL CHECK (purpose IN ('email_ok', 'code_ok')),
    code_hash  text,
    attempts   integer NOT NULL DEFAULT 0,
    used_at    timestamptz,
    expires_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX login_tokens_expires_at_idx ON login_tokens (expires_at);
