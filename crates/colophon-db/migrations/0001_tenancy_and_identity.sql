-- Milestone P1, step 1.1: who you are (identities, sessions, sign-in attempts) and where you
-- may work (projects, environments, memberships). docs/modules/02-db.md, 03-auth.md.
--
-- The application role never owns a table and never bypasses row-level security. Every
-- project-scoped table denies everything unless the transaction has set its context with
-- set_config('app.identity_id' | 'app.project_id', ..., true), which lasts only until the
-- transaction ends, so a pooled connection can't carry one request's context into the next.

-- Production creates its login roles separately; this only makes sure the privilege role exists.
DO $$
BEGIN
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'colophon_api') THEN
        CREATE ROLE colophon_api NOLOGIN NOSUPERUSER NOBYPASSRLS;
    END IF;
END
$$;

-- A person, as an identity provider vouches for them. Identity is (issuer, subject); an email
-- address is never used to decide who someone is (module 03).
CREATE TABLE identities (
    id uuid PRIMARY KEY,
    issuer text NOT NULL CHECK (length(issuer) BETWEEN 1 AND 512),
    subject text NOT NULL CHECK (length(subject) BETWEEN 1 AND 255),
    display_name text CHECK (length(display_name) <= 200),
    created_at timestamptz NOT NULL DEFAULT now(),
    last_sign_in_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (issuer, subject)
);

-- Opaque session tokens; only their SHA-256 hashes are stored, so a database leak hands out no
-- usable sessions. The CSRF token is stored the same way.
CREATE TABLE sessions (
    id uuid PRIMARY KEY,
    token_hash bytea NOT NULL UNIQUE CHECK (length(token_hash) = 32),
    csrf_hash bytea NOT NULL CHECK (length(csrf_hash) = 32),
    identity_id uuid NOT NULL REFERENCES identities (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    last_seen_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    revoked_at timestamptz,
    CHECK (expires_at > created_at)
);
CREATE INDEX sessions_by_identity ON sessions (identity_id);

-- The state of an OpenID Connect sign-in between leaving for the identity provider and coming
-- back: the state (hashed), the nonce and the PKCE verifier. One use, ten minutes at most.
CREATE TABLE sign_in_attempts (
    state_hash bytea PRIMARY KEY CHECK (length(state_hash) = 32),
    nonce text NOT NULL CHECK (length(nonce) BETWEEN 16 AND 128),
    pkce_verifier text NOT NULL CHECK (length(pkce_verifier) BETWEEN 43 AND 128),
    return_to text NOT NULL CHECK (return_to LIKE '/%' AND return_to NOT LIKE '//%' AND length(return_to) <= 2048),
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL
);
CREATE INDEX sign_in_attempts_by_expiry ON sign_in_attempts (expires_at);

CREATE TABLE projects (
    id uuid PRIMARY KEY,
    slug text NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'),
    name text NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
    created_at timestamptz NOT NULL DEFAULT now()
);

-- epoch and generation advance with releases (milestone P2).
CREATE TABLE environments (
    project_id uuid NOT NULL REFERENCES projects (id),
    id uuid NOT NULL,
    slug text NOT NULL CHECK (slug ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'),
    epoch bigint NOT NULL DEFAULT 1 CHECK (epoch >= 1),
    generation bigint NOT NULL DEFAULT 0 CHECK (generation >= 0),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, id),
    UNIQUE (project_id, slug)
);

CREATE TABLE memberships (
    project_id uuid NOT NULL REFERENCES projects (id),
    identity_id uuid NOT NULL REFERENCES identities (id),
    role text NOT NULL CHECK (role IN ('owner', 'developer', 'editor', 'publisher')),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, identity_id)
);
CREATE INDEX memberships_by_identity ON memberships (identity_id);

-- Row-level security: deny unless the context allows. FORCE applies it to the table owner too.
ALTER TABLE projects ENABLE ROW LEVEL SECURITY;
ALTER TABLE projects FORCE ROW LEVEL SECURITY;
ALTER TABLE environments ENABLE ROW LEVEL SECURITY;
ALTER TABLE environments FORCE ROW LEVEL SECURITY;
ALTER TABLE memberships ENABLE ROW LEVEL SECURITY;
ALTER TABLE memberships FORCE ROW LEVEL SECURITY;

-- A project is visible inside its own context, or to its members.
CREATE POLICY projects_read ON projects FOR SELECT TO colophon_api USING (
    id = nullif(current_setting('app.project_id', true), '')::uuid
    OR EXISTS (
        SELECT 1 FROM memberships m
        WHERE m.project_id = projects.id
          AND m.identity_id = nullif(current_setting('app.identity_id', true), '')::uuid
    )
);
CREATE POLICY projects_create ON projects FOR INSERT TO colophon_api
    WITH CHECK (id = nullif(current_setting('app.project_id', true), '')::uuid);

CREATE POLICY environments_in_project ON environments FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);

CREATE POLICY memberships_read ON memberships FOR SELECT TO colophon_api USING (
    identity_id = nullif(current_setting('app.identity_id', true), '')::uuid
    OR project_id = nullif(current_setting('app.project_id', true), '')::uuid
);
CREATE POLICY memberships_create ON memberships FOR INSERT TO colophon_api
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);

-- Least privilege: no UPDATE or DELETE on projects and memberships yet; sessions are revoked by
-- setting revoked_at, never deleted; sign-in attempts are deleted when used or expired.
GRANT SELECT, INSERT, UPDATE (display_name, last_sign_in_at) ON identities TO colophon_api;
GRANT SELECT, INSERT, UPDATE (last_seen_at, revoked_at) ON sessions TO colophon_api;
GRANT SELECT, INSERT, DELETE ON sign_in_attempts TO colophon_api;
GRANT SELECT, INSERT ON projects TO colophon_api;
GRANT SELECT, INSERT ON environments TO colophon_api;
GRANT SELECT, INSERT ON memberships TO colophon_api;
