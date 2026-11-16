-- D-153: magic-link abuse bounds — a cooldown per address, a ceiling per project, and a place
-- in the table-retention sweep.
--
-- A link request is unauthenticated and sends mail, so without bounds anyone could fill a
-- visitor's inbox, spend a project's mail quota, and grow `audience_magic_links` without end
-- (nothing ever deleted a row). `colophon_db::audience::request_magic_link` now refuses, with
-- a `429` and `Retry-After`, a second link for an address while its last one is under a minute
-- old and still unused, and any link once the project has recorded its ceiling in the last
-- minute; both counts read this table under the requested address's advisory lock, so two
-- requests for one address can't both pass. The indexes below are what those counts and the
-- sweep lean on.
--
-- A link is dead fifteen minutes after it was made (`expires_at`) and its audit event is the
-- record that outlives it, so the sweep deletes links past `expires_at` (by the `attempt`
-- age, like `sign_in_attempts` and `setup_attempts`), through a definer function owned by
-- `colophon_retention` — the worker executes the bounded delete and still can't read a row.

CREATE INDEX audience_magic_links_by_age ON audience_magic_links (project_id, created_at);
CREATE INDEX audience_magic_links_by_expiry ON audience_magic_links (expires_at);

CREATE POLICY audience_magic_links_for_retention ON audience_magic_links FOR ALL
    TO colophon_retention USING (true) WITH CHECK (true);
GRANT SELECT, DELETE ON audience_magic_links TO colophon_retention;

CREATE FUNCTION colophon_sweep_magic_links(cutoff timestamptz, batch bigint)
    RETURNS bigint LANGUAGE plpgsql VOLATILE SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE removed bigint;
BEGIN
    WITH doomed AS (
        SELECT project_id, id FROM public.audience_magic_links
        WHERE expires_at < cutoff ORDER BY expires_at LIMIT batch)
    DELETE FROM public.audience_magic_links l USING doomed d
        WHERE l.project_id = d.project_id AND l.id = d.id;
    GET DIAGNOSTICS removed = ROW_COUNT;
    RETURN removed;
END $$;

ALTER FUNCTION colophon_sweep_magic_links(timestamptz, bigint) OWNER TO colophon_retention;
REVOKE ALL ON FUNCTION colophon_sweep_magic_links(timestamptz, bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_sweep_magic_links(timestamptz, bigint) TO colophon_worker;
