-- Permission management (`00-init` §5.1): a role is a named bundle of
-- permissions; a user holds roles either globally (team_id NULL) or within
-- a team. The actor loaded for a request is the union of its grants'
-- permissions, expanded by the access layer.

-- A permission code is a `resource.action` pair, e.g. `stores.view` or
-- `*.view`; the access layer validates the shape before insert.
CREATE TABLE permissions (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    code        text NOT NULL UNIQUE,
    description text NOT NULL DEFAULT ''
);

CREATE TABLE roles (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    key         text NOT NULL UNIQUE,
    name        text NOT NULL,
    description text NOT NULL DEFAULT ''
);

CREATE TABLE role_permissions (
    role_id         uuid NOT NULL REFERENCES roles (id) ON DELETE CASCADE,
    permission_id   uuid NOT NULL REFERENCES permissions (id) ON DELETE CASCADE,
    PRIMARY KEY (role_id, permission_id)
);

CREATE TABLE user_roles (
    user_id     uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role_id     uuid NOT NULL REFERENCES roles (id) ON DELETE CASCADE,
    team_id     uuid REFERENCES teams (id) ON DELETE CASCADE,
    granted_at  timestamptz NOT NULL DEFAULT now(),
    -- team_id NULL = global grant. NULLs are distinct in a plain UNIQUE,
    -- so `NULLS NOT DISTINCT` is required to keep one row per
    -- (user, role, team) combination including the global one.
    UNIQUE NULLS NOT DISTINCT (user_id, role_id, team_id)
);

CREATE INDEX user_roles_role_id_idx ON user_roles (role_id);
CREATE INDEX user_roles_team_id_idx ON user_roles (team_id);
