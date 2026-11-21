-- NON-SUPERUSER SUBSTITUTE (D-234). Run instead of migrations/0011_delivery_environment.sql only when the migrating role is not a
-- superuser (Neon's owner role, for one): PostgreSQL lets only a superuser, or a role granted SET on the parameter,
-- attach `SET app.project_id = ''` (an undeclared, custom setting) to a function, and Neon allows neither. This file
-- is the original byte for byte except that each such clause is replaced by the same guarantee written in the body:
-- the caller's value is captured on entry, the function sets its own, and every normal return path puts the
-- caller's value back. An error needs nothing: a value set with set_config(..., true) is rolled back with the
-- transaction, or with the savepoint that caught the error, exactly as the clause's would be. An unset value comes
-- back as '' (every reader takes nullif(..., '')). The database records the ORIGINAL file's checksum
-- (colophon_db::nonsuperuser), so it looks the same to sqlx and `admin migrate-status` either way.
--
-- Milestone P3, step 3.1: a cache that can't serve the wrong thing. docs/modules/06-delivery.md,
-- D-048.
--
-- Every public read resolves its environment's current generation from Postgres before it asks
-- the cache, so no cache can bring an older release back (DL3). As a delivery transaction that
-- took five round trips (begin, switch role, set the project, read, commit): about as much as a
-- cached answer saved. This function does it in one statement, with no more privilege than the
-- delivery role has: it runs as colophon_delivery, inside the requested project's context (its
-- own SET clause keeps that context from outliving the call), and returns only the environment's
-- id, epoch and generation.

CREATE FUNCTION delivery_environment(project uuid, environment_slug text)
    RETURNS TABLE (id uuid, epoch bigint, generation bigint)
    LANGUAGE plpgsql
    VOLATILE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    caller_project_id text := current_setting('app.project_id', true);
BEGIN
    PERFORM set_config('app.project_id', project::text, true);
    RETURN QUERY
        SELECT e.id, e.epoch, e.generation
        FROM public.environments e
        WHERE e.project_id = project AND e.slug = environment_slug;
    PERFORM set_config('app.project_id', coalesce(caller_project_id, ''), true);
END
$$;

ALTER FUNCTION delivery_environment(uuid, text) OWNER TO colophon_delivery;
REVOKE ALL ON FUNCTION delivery_environment(uuid, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION delivery_environment(uuid, text) TO colophon_api;
