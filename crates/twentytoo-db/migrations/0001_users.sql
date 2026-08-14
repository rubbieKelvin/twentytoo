-- Core identity: the authenticated principal (`00` §6.1).
--
-- Emails are stored lowercase; the access layer normalizes on write and
-- lookup. `password_hash` is NULL until the user sets one (invite flow) —
-- hashing itself is the auth module's job, this table only stores the
-- opaque result. `status` gates sign-in: the auth flow must reject
-- `invited` (no password yet) and `disabled` accounts.
CREATE TABLE users (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    email         text NOT NULL UNIQUE,
    name          text NOT NULL,
    password_hash text,
    status        text NOT NULL DEFAULT 'active'
                  CHECK (status IN ('active', 'invited', 'disabled')),
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now()
);
