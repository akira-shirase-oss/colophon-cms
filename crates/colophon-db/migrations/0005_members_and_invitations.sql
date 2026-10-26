-- Milestone P1, step 1.7: managing members, and invitations. docs/modules/03-auth.md ("Roles and
-- memberships", "Invitations"), D-042.
--
-- Owners can change a member's role or remove them, inside the project's context. A project
-- always keeps an owner: the application locks the owners before changing one, so concurrent
-- changes queue, and a deferred trigger refuses any commit that would leave none.
--
-- An invitation is a single-use link for one role in one project, valid 72 hours by default,
-- stored as a SHA-256 hash. The person it reaches signs in, sees the project and the role, and
-- accepts; the membership is theirs, whatever identity they sign in with.

CREATE POLICY memberships_change ON memberships FOR UPDATE TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
CREATE POLICY memberships_remove ON memberships FOR DELETE TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
GRANT UPDATE (role), DELETE ON memberships TO colophon_api;

CREATE FUNCTION memberships_keep_an_owner() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM memberships WHERE project_id = OLD.project_id AND role = 'owner') THEN
        RAISE EXCEPTION 'a project must keep at least one owner' USING ERRCODE = 'check_violation';
    END IF;
    RETURN NULL;
END;
$$;
CREATE CONSTRAINT TRIGGER memberships_keep_an_owner
    AFTER UPDATE OR DELETE ON memberships
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION memberships_keep_an_owner();

-- Not project-scoped for the person accepting (they aren't a member yet): reached only through
-- colophon_db::members, by token hash or from an owner's project transaction.
CREATE TABLE invitations (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES projects (id),
    token_hash bytea NOT NULL UNIQUE CHECK (length(token_hash) = 32),
    role text NOT NULL CHECK (role IN ('owner', 'developer', 'editor', 'publisher')),
    -- Who it's for, as the owner wrote it; never used to decide identity (A3).
    label text NOT NULL CHECK (char_length(label) BETWEEN 1 AND 100),
    invited_by uuid NOT NULL REFERENCES identities (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL CHECK (expires_at > created_at),
    accepted_by uuid REFERENCES identities (id),
    accepted_at timestamptz,
    revoked_at timestamptz,
    CHECK ((accepted_by IS NULL) = (accepted_at IS NULL)),
    CHECK (accepted_at IS NULL OR revoked_at IS NULL)
);
CREATE INDEX invitations_by_project ON invitations (project_id, created_at);

GRANT SELECT, INSERT, UPDATE (accepted_by, accepted_at, revoked_at) ON invitations TO colophon_api;
