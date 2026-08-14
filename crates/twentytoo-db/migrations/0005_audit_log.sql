-- Audit trail (`00-init` §5.5): every mutation and action invocation is
-- recorded with the actor, the affected resource + record, and the
-- before/after state. Append-only and immutable by design — the access
-- layer only inserts and selects, and `actor_id`/`actor_email` are text
-- snapshots so entries survive user deletion.
CREATE TABLE audit_log (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    actor_id      text NOT NULL,
    actor_email   text NOT NULL,
    action        text NOT NULL
                  CHECK (
                    action IN (
                        'create',
                        'update',
                        'delete',
                        'execute',
                        'login',
                        'logout',
                        'impersonate'
                        )
                    ),
    resource      text NOT NULL,
    resource_id   text NOT NULL,
    before        jsonb,
    after         jsonb,
    ip            text,
    created_at    timestamptz NOT NULL DEFAULT now()
);

-- Per-record history (the detail-view audit tab), per-actor, and global
-- reverse-chronological listings.
CREATE INDEX audit_log_resource_record_idx ON audit_log (resource, resource_id, created_at);
CREATE INDEX audit_log_actor_idx ON audit_log (actor_id, created_at);
CREATE INDEX audit_log_created_at_idx ON audit_log (created_at);
