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
    SET app.project_id = ''
AS $$
BEGIN
    PERFORM set_config('app.project_id', project::text, true);
    RETURN QUERY
        SELECT e.id, e.epoch, e.generation
        FROM public.environments e
        WHERE e.project_id = project AND e.slug = environment_slug;
END
$$;

ALTER FUNCTION delivery_environment(uuid, text) OWNER TO colophon_delivery;
REVOKE ALL ON FUNCTION delivery_environment(uuid, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION delivery_environment(uuid, text) TO colophon_api;
