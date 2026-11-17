-- D-168: the owner guards' functions pin their search_path, and the existence check answers
-- only its trigger.
--
-- 0058's three trigger functions ran on the caller's search_path, and the last-owner check
-- called colophon_project_exists unqualified. Nothing exploits that today — the application
-- can't create objects in any schema — but a guard shouldn't depend on it, so each now pins
-- `pg_catalog, public` and names what it calls by schema.
--
-- colophon_project_exists sees every project's row past row-level security, and colophon_api
-- may execute it (the last-owner trigger runs as the application), so any query could ask it
-- whether a project id exists. It now answers only inside a trigger. It belongs to
-- colophon_deleter, so it is dropped as its owner and created again with the same owner and
-- grant, as 0060 and 0061 do.

ALTER FUNCTION colophon_audit_append_only() SET search_path = pg_catalog, public;
ALTER FUNCTION colophon_owner_role_is_fixed() SET search_path = pg_catalog, public;

SET LOCAL ROLE colophon_deleter;
DROP FUNCTION colophon_project_exists(uuid);
RESET ROLE;

CREATE FUNCTION colophon_project_exists(project uuid) RETURNS boolean
    LANGUAGE plpgsql STABLE SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    IF pg_trigger_depth() = 0 THEN
        RAISE EXCEPTION 'colophon_project_exists answers only the last-owner trigger'
            USING ERRCODE = 'insufficient_privilege';
    END IF;
    RETURN EXISTS (SELECT 1 FROM public.projects p WHERE p.id = project);
END
$$;
ALTER FUNCTION colophon_project_exists(uuid) OWNER TO colophon_deleter;
REVOKE ALL ON FUNCTION colophon_project_exists(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_project_exists(uuid) TO colophon_api;

CREATE OR REPLACE FUNCTION memberships_keep_an_owner() RETURNS trigger LANGUAGE plpgsql
    SET search_path = pg_catalog, public
AS $$
BEGIN
    -- A deleted project loses its members outright: with its row gone there is nothing to
    -- keep an owner of. A ledger replay may remove the last owner of a project its backup
    -- predates — only while the replaying transaction is still `colophon_deleter`.
    IF public.colophon_project_exists(OLD.project_id)
       AND NOT (current_user = 'colophon_deleter'
                AND current_setting('app.ledger_replay', true) = 'on')
       AND NOT EXISTS (SELECT 1 FROM public.memberships
                       WHERE project_id = OLD.project_id AND role = 'owner') THEN
        RAISE EXCEPTION 'a project must keep at least one owner' USING ERRCODE = 'check_violation';
    END IF;
    RETURN NULL;
END
$$;
