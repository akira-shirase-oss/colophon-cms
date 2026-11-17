-- The library's search, served by its trigram index (D-175 follow-up; docs/modules/07-media.md).
--
-- Under colophon_api's row security `ILIKE` isn't leakproof, so the planner may not use an
-- index condition built from it before the policy's own check: a search over `assets` scanned
-- every row of the environment (65–150 ms at 50,000 on the laptop) while the owner's same query
-- took 1 ms through `assets_search_trgm`. So the searchable text — and the two filters a search
-- combines with — live in `asset_search`, which has no row security and no grants: only
-- `colophon_asset_search` reads it, as its owner, and it scopes every read to the project the
-- transaction names in `app.project_id` — the very setting the `assets` policy trusts — so the
-- boundary is the one row security already draws. A trigger keeps the copy in step with the
-- asset in the same transaction that changes it.

CREATE TABLE asset_search (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    asset_id uuid NOT NULL,
    created_at timestamptz NOT NULL,
    state text NOT NULL,
    tags text[] NOT NULL,
    search text NOT NULL,
    PRIMARY KEY (project_id, environment_id, asset_id),
    FOREIGN KEY (project_id, environment_id, asset_id)
        REFERENCES assets (project_id, environment_id, id) ON DELETE CASCADE
);
CREATE INDEX asset_search_trgm ON asset_search USING gin (search gin_trgm_ops);
CREATE INDEX asset_search_newest
    ON asset_search (project_id, environment_id, created_at DESC, asset_id DESC);
REVOKE ALL ON asset_search FROM PUBLIC;

INSERT INTO asset_search (project_id, environment_id, asset_id, created_at, state, tags, search)
SELECT project_id, environment_id, id, created_at, state, tags, search FROM assets;

-- The copy follows every row the API, the worker or an operator writes.
CREATE FUNCTION colophon_asset_search_sync() RETURNS trigger
    LANGUAGE plpgsql
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    INSERT INTO public.asset_search (project_id, environment_id, asset_id, created_at, state,
                                     tags, search)
    VALUES (NEW.project_id, NEW.environment_id, NEW.id, NEW.created_at, NEW.state, NEW.tags,
            NEW.search)
    ON CONFLICT (project_id, environment_id, asset_id) DO UPDATE
        SET state = EXCLUDED.state, tags = EXCLUDED.tags, search = EXCLUDED.search;
    RETURN NULL;
END
$$;
REVOKE ALL ON FUNCTION colophon_asset_search_sync() FROM PUBLIC;
CREATE TRIGGER assets_search_sync AFTER INSERT OR UPDATE OF state, tags, file_name, title, alt
    ON assets FOR EACH ROW EXECUTE FUNCTION colophon_asset_search_sync();

-- A page of the transaction's project's matches, newest first, after the keyset position when
-- given: what `GET {P}/assets?q=` reads before it loads the rows themselves through `assets`
-- (and its row security). `pattern` is an ILIKE pattern with `\` as its escape.
CREATE FUNCTION colophon_asset_search(environment uuid, pattern text, wanted_state text,
                                      wanted_tag text, after_at timestamptz, after_id uuid,
                                      most integer)
    RETURNS TABLE (asset_id uuid)
    LANGUAGE sql
    STABLE
    -- At most a page: the planner then joins its ids to `assets` by key, not by a scan.
    ROWS 101
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
    SELECT s.asset_id
    FROM public.asset_search s
    WHERE s.project_id = nullif(current_setting('app.project_id', true), '')::uuid
      AND s.environment_id = environment
      AND s.search ILIKE pattern ESCAPE '\'
      AND (wanted_state IS NULL OR s.state = wanted_state)
      AND (wanted_tag IS NULL OR s.tags @> ARRAY[wanted_tag])
      AND (after_at IS NULL OR (s.created_at, s.asset_id) < (after_at, after_id))
    ORDER BY s.created_at DESC, s.asset_id DESC
    LIMIT least(greatest(most, 1), 101)
$$;
REVOKE ALL ON FUNCTION colophon_asset_search(uuid, text, text, text, timestamptz, uuid, integer)
    FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_asset_search(uuid, text, text, text, timestamptz, uuid, integer)
    TO colophon_api;

-- The assets table's own trigram index served nothing the API could use; the copy's does.
DROP INDEX assets_search_trgm;
