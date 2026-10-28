-- Milestone R, step R.4: API tokens and the audit log. docs/modules/03-auth.md, D-058, D-063.
--
-- An API token belongs to one project and holds permissions the way a role does: read-only,
-- full access, or a custom list. It acts as an identity of its own, so everything it writes is
-- attributed to it through the columns that already name an identity (created_by, author_id).
-- Only the SHA-256 hash of its secret is stored, as for sessions and personal tokens. No token
-- may manage members, roles or tokens, or read the audit log: the database refuses those
-- permissions as well as the API.
--
-- The audit log records every change to roles, memberships, invitations and tokens, in the
-- transaction that makes it. It is append-only: the API may insert and read, never update or
-- delete, and a trigger refuses updates and deletes from anyone. Retention (90 days by default,
-- spec §21) arrives with operations in P6.

-- Owners may read the audit log: a new action, held by every owner role, new and existing.
ALTER TABLE role_permissions DROP CONSTRAINT role_permissions_action_check;
ALTER TABLE role_permissions ADD CONSTRAINT role_permissions_action_check CHECK (action IN (
    'content.read', 'content.create', 'content.update', 'content.delete', 'content.review',
    'content.publish', 'schema.change', 'members.manage', 'roles.manage',
    'destinations.manage', 'tokens.manage', 'audit.read'));

CREATE OR REPLACE FUNCTION colophon_seed_roles(project uuid) RETURNS void LANGUAGE sql AS $$
    INSERT INTO roles (project_id, key, name, description, built_in) VALUES
        (project, 'owner', 'Owner', 'Configures the project, its members and its roles.', true),
        (project, 'developer', 'Developer', 'Manages content types and website connections.', true),
        (project, 'editor', 'Editor', 'Writes and edits content.', true),
        (project, 'publisher', 'Publisher', 'Writes content, reviews it and publishes it.', true);
    INSERT INTO role_permissions (project_id, role_key, action)
    SELECT project, role_key, action FROM (VALUES
        ('owner', 'content.read'), ('owner', 'content.create'), ('owner', 'content.update'),
        ('owner', 'content.delete'), ('owner', 'schema.change'), ('owner', 'destinations.manage'),
        ('owner', 'members.manage'), ('owner', 'roles.manage'), ('owner', 'tokens.manage'),
        ('owner', 'audit.read'), ('owner', 'content.review'), ('owner', 'content.publish'),
        ('developer', 'content.read'), ('developer', 'content.create'), ('developer', 'content.update'),
        ('developer', 'schema.change'), ('developer', 'destinations.manage'),
        ('editor', 'content.read'), ('editor', 'content.create'), ('editor', 'content.update'),
        ('publisher', 'content.read'), ('publisher', 'content.create'), ('publisher', 'content.update'),
        ('publisher', 'content.review'), ('publisher', 'content.publish')
    ) AS builtin (role_key, action);
$$;

ALTER TABLE roles NO FORCE ROW LEVEL SECURITY;
ALTER TABLE role_permissions NO FORCE ROW LEVEL SECURITY;
INSERT INTO role_permissions (project_id, role_key, action)
SELECT project_id, 'owner', 'audit.read' FROM roles WHERE key = 'owner'
ON CONFLICT DO NOTHING;
ALTER TABLE roles FORCE ROW LEVEL SECURITY;
ALTER TABLE role_permissions FORCE ROW LEVEL SECURITY;

-- API tokens.
CREATE TABLE api_tokens (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES projects (id),
    -- The token's own identity (issuer `urn:colophon:api-token`, subject its id): what it writes
    -- names this identity, and the audit log names it as the actor.
    identity_id uuid NOT NULL UNIQUE REFERENCES identities (id),
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 100),
    description text NOT NULL DEFAULT '' CHECK (char_length(description) <= 500),
    kind text NOT NULL CHECK (kind IN ('read-only', 'full-access', 'custom')),
    token_hash bytea NOT NULL UNIQUE CHECK (length(token_hash) = 32),
    created_by uuid NOT NULL REFERENCES identities (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    -- A year and a day at most (colophon_domain::policy::MAX_TOKEN_DAYS).
    expires_at timestamptz NOT NULL
        CHECK (expires_at > created_at AND expires_at <= created_at + interval '366 days'),
    last_used_at timestamptz,
    revoked_at timestamptz,
    revoked_by uuid REFERENCES identities (id),
    CHECK ((revoked_at IS NULL) = (revoked_by IS NULL)),
    UNIQUE (project_id, id)
);
CREATE INDEX api_tokens_by_project ON api_tokens (project_id, created_at DESC);

CREATE TABLE api_token_permissions (
    project_id uuid NOT NULL,
    token_id uuid NOT NULL,
    -- Never the actions that manage access or read the audit log (TOKEN_FORBIDDEN).
    action text NOT NULL CHECK (action IN (
        'content.read', 'content.create', 'content.update', 'content.delete', 'content.review',
        'content.publish', 'schema.change', 'destinations.manage')),
    subject text NOT NULL DEFAULT '*'
        CHECK (subject = '*' OR subject ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'),
    fields text[] CHECK (fields IS NULL OR (cardinality(fields) BETWEEN 1 AND 100
        AND array_to_string(fields, ',') ~ '^[a-z][a-z0-9_]{0,63}(,[a-z][a-z0-9_]{0,63})*$')),
    condition text CHECK (condition IN ('own')),
    CHECK (fields IS NULL OR action IN ('content.read', 'content.create', 'content.update')),
    CHECK (condition IS NULL OR action IN ('content.read', 'content.update', 'content.delete')),
    FOREIGN KEY (project_id, token_id) REFERENCES api_tokens (project_id, id)
);
CREATE UNIQUE INDEX api_token_permissions_key
    ON api_token_permissions (project_id, token_id, action, subject, (coalesce(condition, '')));

-- Row-level security binds both to the project's context. api_tokens' is enabled, not forced,
-- so the lookup below (owned by the migration login) can find a token by its hash before its
-- project is known; the API's own role only ever sees its project's tokens.
ALTER TABLE api_tokens ENABLE ROW LEVEL SECURITY;
ALTER TABLE api_token_permissions ENABLE ROW LEVEL SECURITY;
ALTER TABLE api_token_permissions FORCE ROW LEVEL SECURITY;
CREATE POLICY api_tokens_in_project ON api_tokens FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
CREATE POLICY api_token_permissions_in_project ON api_token_permissions FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);

-- The one way to find a token by its secret's hash: what it is, never its permissions (those are
-- read inside the project's context, as a member's are). Revoked and expired tokens are found
-- too, so the API can say which; it refuses them.
CREATE FUNCTION colophon_api_token(hash bytea)
    RETURNS TABLE (id uuid, project_id uuid, identity_id uuid, expires_at timestamptz,
                   revoked_at timestamptz, last_used_at timestamptz)
    LANGUAGE sql
    STABLE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
    SELECT t.id, t.project_id, t.identity_id, t.expires_at, t.revoked_at, t.last_used_at
    FROM public.api_tokens t
    WHERE t.token_hash = hash;
$$;
REVOKE ALL ON FUNCTION colophon_api_token(bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_api_token(bytea) TO colophon_api;

-- Tokens are revoked, never deleted: the audit log and what they wrote keep naming them.
GRANT SELECT, INSERT, UPDATE (last_used_at, revoked_at, revoked_by) ON api_tokens TO colophon_api;
GRANT SELECT, INSERT ON api_token_permissions TO colophon_api;

-- The audit log.
CREATE TABLE audit_events (
    project_id uuid NOT NULL REFERENCES projects (id),
    -- Time-ordered (UUIDv7), so the newest come first by id.
    id uuid NOT NULL,
    at timestamptz NOT NULL DEFAULT now(),
    -- Who did it: a person, or an API token's identity.
    actor_id uuid NOT NULL REFERENCES identities (id),
    -- `role.created`, `member.role_changed`, `token.revoked`, …
    action text NOT NULL CHECK (action ~ '^[a-z]+\.[a-z_]+$' AND char_length(action) <= 50),
    -- What it was done to: a role's key, an identity's id, an invitation's or a token's id.
    subject text NOT NULL CHECK (char_length(subject) BETWEEN 1 AND 200),
    -- Before and after, as the API showed them; never a secret.
    detail jsonb NOT NULL DEFAULT '{}' CHECK (octet_length(detail::text) <= 16384),
    PRIMARY KEY (project_id, id)
);
CREATE INDEX audit_events_by_actor ON audit_events (project_id, actor_id, id DESC);

ALTER TABLE audit_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE audit_events FORCE ROW LEVEL SECURITY;
CREATE POLICY audit_events_in_project ON audit_events FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);

CREATE FUNCTION colophon_audit_append_only() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'the audit log is append-only: % refused', tg_op
        USING ERRCODE = 'insufficient_privilege';
END
$$;
CREATE TRIGGER audit_events_append_only BEFORE UPDATE OR DELETE ON audit_events
    FOR EACH ROW EXECUTE FUNCTION colophon_audit_append_only();
CREATE TRIGGER audit_events_no_truncate BEFORE TRUNCATE ON audit_events
    FOR EACH STATEMENT EXECUTE FUNCTION colophon_audit_append_only();

GRANT SELECT, INSERT ON audit_events TO colophon_api;
