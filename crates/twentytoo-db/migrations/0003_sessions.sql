-- Server-side session store (`01` §10.6): the cookie carries a random
-- token, the table stores only its SHA-256 hash — a leaked table never
-- yields usable session credentials.
--
-- `team_id` is the team this session acts within (multi-tenant); NULL
-- means the session has no team scope and only global role grants apply.
CREATE TABLE sessions (
    token_hash    text PRIMARY KEY,
    user_id       uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    team_id       uuid REFERENCES teams (id) ON DELETE SET NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    expires_at    timestamptz NOT NULL,
    last_seen_at  timestamptz,
    user_agent    text,
    ip            text
);

CREATE INDEX sessions_user_id_idx ON sessions (user_id);
CREATE INDEX sessions_expires_at_idx ON sessions (expires_at);
