-- The audit trail: a canonical, append-only in-app event stream. The
-- access layer only inserts and selects — events are immutable by
-- design.
--
-- `type` is an open `resource.action` discriminator (`stores.created`,
-- `users.login`, …) — deliberately not a CHECK-constrained set, so
-- types evolve additively without schema changes. `actor` and `target`
-- are point-in-time resource envelopes:
--
--     { "type": "<kind>", "properties": { … scalar snapshot … } }
--
-- so history survives actor deletion and record renames. `properties`
-- is the type-specific payload (before/after state for mutations);
-- `context` carries request metadata (client IP, and later user agent /
-- route). No global sequence number: ordering is per query, by
-- (timestamp, id).
CREATE TABLE inapp_events (
    id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    timestamp  timestamptz NOT NULL DEFAULT now(),
    type       text NOT NULL,
    actor      jsonb NOT NULL,
    target     jsonb NOT NULL,
    properties jsonb NOT NULL,
    context    jsonb NOT NULL DEFAULT '{}'::jsonb
);

-- The global timeline and per-type listings.
CREATE INDEX inapp_events_timestamp_idx ON inapp_events (timestamp DESC, id DESC);
CREATE INDEX inapp_events_type_timestamp_idx ON inapp_events (type, timestamp DESC, id DESC);
