-- Milestone P7, step 7.1: the site's own people, and the audience they read as.
-- docs/modules/03-auth.md ("Audience"), docs/modules/06-delivery.md ("Audience visibility"),
-- D-137.
--
-- A site visitor is not a member: members manage a project (staff sign-in, OIDC or
-- email-and-password), visitors read what it publishes. `site_users` are per project — the
-- same email is a different visitor in two projects, and nothing a visitor does ever touches a
-- management route. They sign up and in with an email and an Argon2id password hash (the same
-- password discipline members have, migration 0032), hold opaque sessions whose hashes alone
-- are stored, and belong to one of the project's audience roles (`site_roles`), which a staff
-- member with the new `audience.manage` permission maintains.
--
-- What a visitor may read is a property of the published revision: `visibility` on `revisions`
-- — 'public', 'authenticated' (any live site session), or 'role:<name>' (a session whose user
-- holds that audience role) — copied into `publications` when a release opens it, exactly like
-- title and content are. Delivery enforces it as row-level security on `colophon_delivery`:
-- the transaction's `app.audience_role` setting (empty for anonymous reads, the visitor's role
-- when they hold a live session) is compared against the row, so gating is the database's,
-- never application filtering (spec §15's private-content rule). Tombstones carry the same
-- visibility so a member's 410 for something they could read doesn't leak to the anonymous.
--
-- `site_users` and `site_sessions` are row-security-enabled but not forced, like `api_tokens`
-- (migration 0023): the owner-side `colophon_audience_session` lookup resolves a session by its
-- token hash before any project context exists, then the API's own project-scoped policies take
-- over. Nothing except the API role and that function may touch them at all.

-- Owners may manage a project's audience: a new action, held by every owner role, new and
-- existing. Tokens may never hold it (TOKEN_FORBIDDEN, like the other access-managing actions).
ALTER TABLE role_permissions DROP CONSTRAINT role_permissions_action_check;
ALTER TABLE role_permissions ADD CONSTRAINT role_permissions_action_check CHECK (action IN (
    'content.read', 'content.create', 'content.update', 'content.delete', 'content.review',
    'content.publish', 'schema.change', 'members.manage', 'roles.manage',
    'destinations.manage', 'tokens.manage', 'audit.read', 'audience.manage'));

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
        ('owner', 'audit.read'), ('owner', 'audience.manage'),
        ('owner', 'content.review'), ('owner', 'content.publish'),
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
SELECT project_id, 'owner', 'audience.manage' FROM roles WHERE key = 'owner'
ON CONFLICT DO NOTHING;
ALTER TABLE roles FORCE ROW LEVEL SECURITY;
ALTER TABLE role_permissions FORCE ROW LEVEL SECURITY;

-- Audience roles: project-scoped labels a site user's role names and 'role:<name>' visibility
-- grants to. Kept deliberately thin — a label is all delivery needs (D-137); permissions and
-- entitlements are a later phase. The name is a role key like staff roles' (1–63 chars of
-- a–z, 0–9 and '-', starting with a letter).
CREATE TABLE site_roles (
    project_id uuid NOT NULL REFERENCES projects (id),
    name text NOT NULL CHECK (name ~ '^[a-z][a-z0-9-]{0,62}$'),
    -- Whether signup may choose it: a visitor assigns themselves only open roles; staff put
    -- users in any role, open or not (the closed ones gate paid and friends-and-family tiers).
    self_assign boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, name)
);

ALTER TABLE site_roles ENABLE ROW LEVEL SECURITY;
ALTER TABLE site_roles FORCE ROW LEVEL SECURITY;
CREATE POLICY site_roles_in_project ON site_roles FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
GRANT SELECT, INSERT, UPDATE, DELETE ON site_roles TO colophon_api;

-- One site's visitor. The Argon2id PHC string is selected only to verify against; lockout
-- state mirrors identities' (migration 0032). `role` is RESTRICTed: a role members hold can't
-- vanish under them (the API says role_in_use).
CREATE TABLE site_users (
    project_id uuid NOT NULL,
    id uuid NOT NULL,
    email text NOT NULL CHECK (char_length(email) BETWEEN 3 AND 254 AND email = lower(email)),
    password_hash text NOT NULL CHECK (char_length(password_hash) BETWEEN 1 AND 255),
    display_name text CHECK (char_length(display_name) <= 200),
    role text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    last_sign_in_at timestamptz,
    failed_sign_ins smallint NOT NULL DEFAULT 0 CHECK (failed_sign_ins >= 0),
    locked_until timestamptz,
    disabled_at timestamptz,
    PRIMARY KEY (project_id, id),
    UNIQUE (project_id, email),
    FOREIGN KEY (project_id) REFERENCES projects (id),
    FOREIGN KEY (project_id, role) REFERENCES site_roles (project_id, name)
);
CREATE INDEX site_users_by_email ON site_users (project_id, email);

ALTER TABLE site_users ENABLE ROW LEVEL SECURITY;
CREATE POLICY site_users_in_project ON site_users FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
GRANT SELECT, INSERT, UPDATE (role, disabled_at, failed_sign_ins, locked_until, last_sign_in_at)
    ON site_users TO colophon_api;

-- A visitor's session: an opaque token; only its SHA-256 hash is stored, as for members'
-- sessions and tokens (migrations 0001, 0023). Revoked sessions are kept for the audit trail;
-- `expires_at` is the absolute end regardless of use.
CREATE TABLE site_sessions (
    project_id uuid NOT NULL,
    id uuid NOT NULL,
    site_user_id uuid NOT NULL,
    token_hash bytea NOT NULL UNIQUE CHECK (length(token_hash) = 32),
    csrf_hash bytea NOT NULL CHECK (length(csrf_hash) = 32),
    created_at timestamptz NOT NULL DEFAULT now(),
    last_seen_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    revoked_at timestamptz,
    PRIMARY KEY (project_id, id),
    CHECK (expires_at > created_at),
    FOREIGN KEY (project_id, site_user_id) REFERENCES site_users (project_id, id)
);
CREATE INDEX site_sessions_by_user ON site_sessions (project_id, site_user_id);

ALTER TABLE site_sessions ENABLE ROW LEVEL SECURITY;
CREATE POLICY site_sessions_in_project ON site_sessions FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
GRANT SELECT, INSERT, UPDATE (last_seen_at, revoked_at), DELETE ON site_sessions TO colophon_api;

-- The one way to find a site session by its token's hash: the session and the user it belongs
-- to, live or not — the API says which (revoked, expired, idle over seven days — the UPDATE
-- bound must match the db layer's IDLE — or a disabled user). As a side effect it stamps
-- `last_seen_at`, at most once a minute like members' sessions, and only for sessions still
-- inside the idle window: a stale last_seen must stay stale, or an idle session would revive
-- itself by being read. Owned by the migrator so it reads both tables before their project
-- context exists (enabled, not forced — see the header); executable by the API alone.
CREATE FUNCTION colophon_audience_session(project uuid, hash bytea)
    RETURNS TABLE (session_id uuid, site_user_id uuid, email text, display_name text,
                   role text, disabled_at timestamptz, created_at timestamptz,
                   revoked_at timestamptz, last_seen_at timestamptz, expires_at timestamptz,
                   csrf_hash bytea)
    LANGUAGE plpgsql
    VOLATILE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    UPDATE public.site_sessions s SET last_seen_at = now()
    WHERE s.project_id = project AND s.token_hash = hash
      AND s.revoked_at IS NULL AND s.expires_at > now()
      AND s.last_seen_at BETWEEN now() - interval '7 days' AND now() - interval '1 minute';
    RETURN QUERY
        SELECT s.id, u.id, u.email, u.display_name, u.role, u.disabled_at, u.created_at,
               s.revoked_at, s.last_seen_at, s.expires_at, s.csrf_hash
        FROM public.site_sessions s
        JOIN public.site_users u ON u.project_id = s.project_id AND u.id = s.site_user_id
        WHERE s.project_id = project AND s.token_hash = hash;
END
$$;

REVOKE ALL ON FUNCTION colophon_audience_session(uuid, bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_audience_session(uuid, bytea) TO colophon_api;

-- Who may read what readers get: the draft's gate moves with saves like its title and slug
-- (`entries.draft_visibility`, denormalized for lists and inheritance), the revision pins it
-- like content does (`revisions.visibility`), a release copies it into the projection
-- (`publications.visibility`), and an unpublish's tombstone keeps it so a member's honest 410
-- doesn't surface to the anonymous. Everything already out is public.
ALTER TABLE entries ADD COLUMN draft_visibility text NOT NULL DEFAULT 'public'
    CHECK (draft_visibility IN ('public', 'authenticated')
           OR draft_visibility ~ '^role:[a-z][a-z0-9-]{0,62}$');
GRANT UPDATE (draft_visibility) ON entries TO colophon_api;

ALTER TABLE revisions ADD COLUMN visibility text NOT NULL DEFAULT 'public'
    CHECK (visibility IN ('public', 'authenticated')
           OR visibility ~ '^role:[a-z][a-z0-9-]{0,62}$');

ALTER TABLE publications ADD COLUMN visibility text NOT NULL DEFAULT 'public'
    CHECK (visibility IN ('public', 'authenticated')
           OR visibility ~ '^role:[a-z][a-z0-9-]{0,62}$');

ALTER TABLE tombstones ADD COLUMN visibility text NOT NULL DEFAULT 'public'
    CHECK (visibility IN ('public', 'authenticated')
           OR visibility ~ '^role:[a-z][a-z0-9-]{0,62}$');

-- The gate itself, as row-level security: anonymous delivery (no `app.audience_role` set in
-- the transaction) sees 'public' only; a transaction begun for a live site session sees
-- 'public', 'authenticated' and the user's own 'role:<name>'. The setting is a plain text
-- claim — the API sets it only after the session lookup proved it — but it decides nothing
-- outside this read, and it expires with the transaction (`set_config …, true`).
DROP POLICY publications_delivered ON publications;
CREATE POLICY publications_delivered ON publications FOR SELECT TO colophon_delivery
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid
           AND (visibility = 'public'
                OR (nullif(current_setting('app.audience_role', true), '') IS NOT NULL
                    AND (visibility = 'authenticated'
                         OR visibility = 'role:' || current_setting('app.audience_role', true)))));

DROP POLICY tombstones_delivered ON tombstones;
CREATE POLICY tombstones_delivered ON tombstones FOR SELECT TO colophon_delivery
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid
           AND (visibility = 'public'
                OR (nullif(current_setting('app.audience_role', true), '') IS NOT NULL
                    AND (visibility = 'authenticated'
                         OR visibility = 'role:' || current_setting('app.audience_role', true)))));
