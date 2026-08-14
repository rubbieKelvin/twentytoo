-- Groups: the grouping boundary (`00` §6.1). Users join groups
-- through `group_members` (many-to-many — a user belongs to any number of
-- groups); groups hold roles through `group_roles` (see
-- `0004_roles_permissions.sql`), which every member inherits.
CREATE TABLE groups (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    name        text NOT NULL,
    slug        text NOT NULL UNIQUE,
    created_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE group_members (
    group_id    uuid NOT NULL REFERENCES groups (id) ON DELETE CASCADE,
    user_id     uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (group_id, user_id)
);

-- Membership is looked up by user (actor load) and by group (member
-- listing); the PK already covers the group-first direction.
CREATE INDEX group_members_user_id_idx ON group_members (user_id);
