-- Team/org: the row-scoping boundary (`00-init` §5.1). Users join teams
-- through `team_members`; roles can additionally be granted per team (see
-- `user_roles`). A deployment that never uses teams simply has empty
-- tables — single-tenant stays the default.
CREATE TABLE teams (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    name        text NOT NULL,
    slug        text NOT NULL UNIQUE,
    created_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE team_members (
    team_id     uuid NOT NULL REFERENCES teams (id) ON DELETE CASCADE,
    user_id     uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (team_id, user_id)
);

-- Team membership is looked up by user (session actor load) and by team
-- (member listing); the PK already covers the team-first direction.
CREATE INDEX team_members_user_id_idx ON team_members (user_id);
