-- D-155: the owner-protecting triggers yield to roles, not to settings anyone can set.
--
-- Migration 0048 let three guards bend for a project's deletion and a ledger replay by
-- reading session settings — `app.deleting_project` and `app.ledger_replay` — and 0050 added
-- `colophon.audit_sweep` for the retention sweep. A setting is not a credential:
-- `set_config` is open to every session, so the application role could set
-- `app.deleting_project` to its own project and then rename the owner role or strip its
-- permissions (`roles`, `role_permissions` writes it is granted), or set `app.ledger_replay`
-- and remove a project's last owner. Each pass now also asks who is running the statement:
--
-- - the owner-role guard yields only to `colophon_deleter`, the role `colophon_delete_project`
--   runs as (SECURITY DEFINER) and the ledger replay switches to (`SET LOCAL ROLE`) — and to
--   the table's own owner, as whom Postgres runs the cascade from `roles` to `role_permissions`;
-- - the audit log's append-only guard yields to `colophon_deleter` for a project's deletion
--   and to `colophon_retention` for the sweep — what the DELETE grants already implied, now
--   said by the trigger itself;
-- - the deferred last-owner check runs at COMMIT, after `colophon_delete_project` has
--   returned, so `current_user` there is the caller's. It yields when the project row itself
--   is gone — a project with no row needs no owner, asked through a definer function that
--   sees past row-level security — and to a replay only while the transaction is still
--   `colophon_deleter`.
--
-- The settings stay as the flags they were — the deleting function still sets its own — but
-- holding one opens nothing any more.

CREATE OR REPLACE FUNCTION colophon_audit_append_only() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    -- The retention sweep: a bounded delete of events past their age, by its own role.
    IF tg_op = 'DELETE' AND current_user = 'colophon_retention'
       AND current_setting('colophon.audit_sweep', true) = 'on' THEN
        RETURN OLD;
    END IF;
    -- A project's deletion takes its trail with it (0048) — only inside the deleting role, and
    -- only rows of the very project being deleted. Nested, not one AND: OLD is unassigned for
    -- the TRUNCATE trigger, and SQL does not promise short-circuit.
    IF tg_op = 'DELETE' AND current_user = 'colophon_deleter' THEN
        IF nullif(current_setting('app.deleting_project', true), '')::uuid = OLD.project_id THEN
            RETURN OLD;
        END IF;
    END IF;
    RAISE EXCEPTION 'the audit log is append-only: % refused', tg_op
        USING ERRCODE = 'insufficient_privilege';
END
$$;

CREATE OR REPLACE FUNCTION colophon_owner_role_is_fixed() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    -- `colophon_deleter` deletes the owner role's row itself; its `role_permissions` go by the
    -- foreign key's cascade, which Postgres runs as the referencing table's owner — so the
    -- table's own owner (the migrator, never the application) passes too. The application
    -- can't reach that cascade: deleting the owner role's row is refused above, as itself.
    IF (current_user = 'colophon_deleter'
        OR (SELECT c.relowner FROM pg_catalog.pg_class c WHERE c.oid = TG_RELID)
           = (SELECT r.oid FROM pg_catalog.pg_roles r WHERE r.rolname = current_user))
       AND nullif(current_setting('app.deleting_project', true), '')::uuid = OLD.project_id THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'the owner role is fixed: it always holds every permission'
        USING ERRCODE = 'insufficient_privilege';
END
$$;

-- Whether a project row exists, seen past row-level security: the deferred last-owner check's
-- one question. It runs as `colophon_deleter`, which may already delete any project in its
-- context and here gains only the sight of every project's row, so a project the caller merely
-- can't see — say, because the transaction moved `app.project_id` after the delete — is never
-- mistaken for one that is gone.
CREATE POLICY projects_deleter_sees ON projects FOR SELECT TO colophon_deleter USING (true);
CREATE FUNCTION colophon_project_exists(project uuid) RETURNS boolean
    LANGUAGE sql STABLE SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
    SELECT EXISTS (SELECT 1 FROM public.projects p WHERE p.id = project)
$$;
ALTER FUNCTION colophon_project_exists(uuid) OWNER TO colophon_deleter;
REVOKE ALL ON FUNCTION colophon_project_exists(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_project_exists(uuid) TO colophon_api;

CREATE OR REPLACE FUNCTION memberships_keep_an_owner() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    -- A deleted project loses its members outright: with its row gone there is nothing to
    -- keep an owner of. A ledger replay may remove the last owner of a project its backup
    -- predates — only while the replaying transaction is still `colophon_deleter`.
    IF colophon_project_exists(OLD.project_id)
       AND NOT (current_user = 'colophon_deleter'
                AND current_setting('app.ledger_replay', true) = 'on')
       AND NOT EXISTS (SELECT 1 FROM public.memberships
                       WHERE project_id = OLD.project_id AND role = 'owner') THEN
        RAISE EXCEPTION 'a project must keep at least one owner' USING ERRCODE = 'check_violation';
    END IF;
    RETURN NULL;
END
$$;
