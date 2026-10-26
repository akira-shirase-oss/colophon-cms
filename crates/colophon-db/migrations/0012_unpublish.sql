-- Milestone P3, step 3.2: taking it back. docs/modules/06-delivery.md ("Unpublish and
-- revocation"), D-049.
--
-- Unpublishing is a change like any other: a changeset item of kind 'unpublish' pins the revision
-- that is published, a reviewer approves exactly that, and publishing the changeset commits a
-- release. For each unpublish item the release closes the entry's open publication, records a
-- tombstone at its generation, and marks every earlier publication of the entry revoked: the
-- origin delivers none of them again, at any generation, so old release links stop working too
-- (spec §15, DL5). History itself stays: the rows remain, for the Studio and the audit trail.
-- Publishing the entry again later opens a new publication after the tombstone.
--
-- Each environment keeps the generation of its latest unpublish. Delivery cache keys name it, so
-- an unpublish retires every answer cached before it, whatever generation it was read at (F07).

ALTER TABLE changeset_items
    ADD COLUMN kind text NOT NULL DEFAULT 'publish' CHECK (kind IN ('publish', 'unpublish'));

ALTER TABLE environments ADD COLUMN last_unpublish bigint NOT NULL DEFAULT 0 CHECK (last_unpublish >= 0);
GRANT UPDATE (last_unpublish) ON environments TO colophon_api;
GRANT SELECT (last_unpublish) ON environments TO colophon_delivery;

-- The release that revoked a publication, if one has: delivery skips revoked rows without a join.
ALTER TABLE publications ADD COLUMN revoked_in bigint CHECK (revoked_in > valid_from);
ALTER TABLE publications ADD FOREIGN KEY (project_id, environment_id, revoked_in)
    REFERENCES releases (project_id, environment_id, generation);
GRANT UPDATE (revoked_in) ON publications TO colophon_api;

CREATE TABLE tombstones (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    entry_id uuid NOT NULL,
    -- The release that unpublished it.
    generation bigint NOT NULL,
    collection text NOT NULL,
    -- What it took down: the revision the changeset pinned and the reviewer approved.
    revision_id uuid NOT NULL,
    -- Also serves "was this entry unpublished at or before generation g" (a 410 rather than a 404).
    PRIMARY KEY (project_id, environment_id, entry_id, generation),
    FOREIGN KEY (project_id, environment_id, generation)
        REFERENCES releases (project_id, environment_id, generation),
    FOREIGN KEY (project_id, environment_id, entry_id, revision_id)
        REFERENCES revisions (project_id, environment_id, entry_id, id)
);
-- What a release took down.
CREATE INDEX tombstones_by_release ON tombstones (project_id, environment_id, generation);

ALTER TABLE tombstones ENABLE ROW LEVEL SECURITY;
ALTER TABLE tombstones FORCE ROW LEVEL SECURITY;
CREATE POLICY tombstones_in_project ON tombstones FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
GRANT SELECT, INSERT ON tombstones TO colophon_api;
CREATE POLICY tombstones_delivered ON tombstones FOR SELECT TO colophon_delivery
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
GRANT SELECT ON tombstones TO colophon_delivery;

-- The per-request lookup also returns the latest unpublish, for the cache key. A changed result
-- type means a new function (CREATE OR REPLACE can't change it); same rules as migration 0011.
DROP FUNCTION delivery_environment(uuid, text);
CREATE FUNCTION delivery_environment(project uuid, environment_slug text)
    RETURNS TABLE (id uuid, epoch bigint, generation bigint, last_unpublish bigint)
    LANGUAGE plpgsql
    VOLATILE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
    SET app.project_id = ''
AS $$
BEGIN
    PERFORM set_config('app.project_id', project::text, true);
    RETURN QUERY
        SELECT e.id, e.epoch, e.generation, e.last_unpublish
        FROM public.environments e
        WHERE e.project_id = project AND e.slug = environment_slug;
END
$$;

ALTER FUNCTION delivery_environment(uuid, text) OWNER TO colophon_delivery;
REVOKE ALL ON FUNCTION delivery_environment(uuid, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION delivery_environment(uuid, text) TO colophon_api;
