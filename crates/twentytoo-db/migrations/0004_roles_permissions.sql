-- Permission management (`00` §6.1): a role is a named bundle of
-- permissions; a user holds roles directly (globally, or scoped to a
-- group) and inherits the roles of every group they belong to. The actor
-- loaded for a request is the union of those grants' permissions,
-- expanded by the access layer.

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

-- A user holding a role: group_id NULL = global grant, non-NULL = the
-- role applies only while acting within that group.
CREATE TABLE user_roles (
    user_id     uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role_id     uuid NOT NULL REFERENCES roles (id) ON DELETE CASCADE,
    group_id    uuid REFERENCES groups (id) ON DELETE CASCADE,
    granted_at  timestamptz NOT NULL DEFAULT now(),
    -- group_id NULL = global grant. NULLs are distinct in a plain UNIQUE,
    -- so `NULLS NOT DISTINCT` keeps one row per (user, role, group)
    -- combination including the global one.
    UNIQUE NULLS NOT DISTINCT (user_id, role_id, group_id)
);

CREATE INDEX user_roles_role_id_idx ON user_roles (role_id);
CREATE INDEX user_roles_group_id_idx ON user_roles (group_id);

-- A role held by a group: every member inherits it, in every context.
CREATE TABLE group_roles (
    group_id    uuid NOT NULL REFERENCES groups (id) ON DELETE CASCADE,
    role_id     uuid NOT NULL REFERENCES roles (id) ON DELETE CASCADE,
    granted_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (group_id, role_id)
);

CREATE INDEX group_roles_role_id_idx ON group_roles (role_id);
