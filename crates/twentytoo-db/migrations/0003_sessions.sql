-- Server-side session store (`00` §6.3): the cookie carries a random
-- token, the table stores only its SHA-256 hash — a leaked table never
-- yields usable session credentials.
--
-- Tracking is deliberately wide: every field except the identity/expiry
-- columns is nullable, and `metadata` is an open JSON object for anything
-- a deployment wants to record (extra headers, device attributes,
-- geolocation, correlation ids). `group_id` is the group this session acts
-- within; NULL means the session has no group scope and only global grants
-- apply.
CREATE TABLE sessions (
    token_hash      text PRIMARY KEY,
    user_id         uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    group_id        uuid REFERENCES groups (id) ON DELETE SET NULL,
    created_at      timestamptz NOT NULL DEFAULT now(),
    expires_at      timestamptz NOT NULL,
    last_seen_at    timestamptz,
    user_agent      text,
    ip              text,
    referrer        text,
    accept_language text,
    device          text,
    os              text,
    browser         text,
    metadata        jsonb NOT NULL DEFAULT '{}'::jsonb
);

CREATE INDEX sessions_user_id_idx ON sessions (user_id);
CREATE INDEX sessions_expires_at_idx ON sessions (expires_at);
