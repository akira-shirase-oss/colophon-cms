-- D-235: self-serve projects. Any signed-in account may create a project for itself, within the
-- per-account cap the server enforces (COLOPHON_PROJECTS_PER_ACCOUNT; administrators: no cap),
-- and only when the installation's registration is open (the server's check: the database can't
-- read the server's configuration). Administrators still create projects on every installation.
--
-- 1. projects.owner_account: the account a project counts against, its creator. The application
--    holds no UPDATE on projects, so it never changes; ownership transfer is out of scope.
-- 2. The restrictive "administrators only" INSERT policy (0006) becomes "for yourself only".
-- 3. colophon_accounts: a NOLOGIN role that reads every project's id and owner_account and
--    nothing else, and owns the definer functions that count across tenants (0093: the projects
--    an account owns, and account_usage's triggers). It holds no other privilege. Its functions
--    come in the next migration: a role owns an object in public only with CREATE there, which
--    D-234's prologue grants a non-superuser's migration for its own transaction, and the role
--    doesn't exist yet when this migration's prologue runs.

-- 3, first: the role (as 0030 makes colophon_deleter). Fresh test databases migrate
-- concurrently, so losing a race to create it is fine.
DO $$
BEGIN
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'colophon_accounts') THEN
        CREATE ROLE colophon_accounts NOLOGIN NOSUPERUSER NOBYPASSRLS;
    END IF;
EXCEPTION WHEN duplicate_object OR unique_violation THEN
    NULL;
END
$$;

-- 1. The owner column, and its backfill: each existing project's earliest owner. A project
-- with no owner membership stays NULL and counts against no one (none exists: the last owner
-- can't leave, 0017). Both tables force row-level security on their owner, and a migrator that
-- isn't a superuser has no BYPASSRLS (D-234): the force is lifted for this one statement and put
-- back, inside this migration's transaction, so no other session ever sees it lifted.
ALTER TABLE projects ADD COLUMN owner_account uuid REFERENCES identities (id);
CREATE INDEX projects_by_owner_account ON projects (owner_account) WHERE owner_account IS NOT NULL;
ALTER TABLE projects NO FORCE ROW LEVEL SECURITY;
ALTER TABLE memberships NO FORCE ROW LEVEL SECURITY;
UPDATE projects p
SET owner_account = (
    SELECT m.identity_id FROM memberships m
    WHERE m.project_id = p.id AND m.role = 'owner'
    ORDER BY m.created_at, m.identity_id
    LIMIT 1
);
ALTER TABLE projects FORCE ROW LEVEL SECURITY;
ALTER TABLE memberships FORCE ROW LEVEL SECURITY;

-- 2. Who may insert a project: anyone, for themselves. The cap and the registration mode are
-- the server's (colophon_db::projects::create and the API).
DROP POLICY projects_created_by_administrators ON projects;
CREATE POLICY projects_created_by_their_owner ON projects AS RESTRICTIVE FOR INSERT TO colophon_api
    WITH CHECK (owner_account = nullif(current_setting('app.identity_id', true), '')::uuid);

-- 3. What colophon_accounts may read: every project's id and owner, nothing else.
CREATE POLICY projects_read_by_accounts ON projects FOR SELECT TO colophon_accounts USING (true);
GRANT SELECT (id, owner_account) ON projects TO colophon_accounts;
