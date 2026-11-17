-- Gap 3: images inside rich text, releases that record the image versions they published, and
-- a library that pages, searches and carries its own words. docs/modules/07-media.md,
-- D-170 to D-173.

-- 1. A rich-text field's images are asset references (D-170): one row per distinct upload the
--    document names, at the place it first names it — up to 200 per document, where an asset
--    list holds at most 100. Everything that reads asset_references (the publish check, the
--    retention keep-rule, the delete refusal, "Used in", 0039's race trigger) covers them as
--    it covers asset fields.
ALTER TABLE asset_references DROP CONSTRAINT asset_references_position_check;
ALTER TABLE asset_references ADD CONSTRAINT asset_references_position_check
    CHECK (position BETWEEN 0 AND 199);

-- 2. What a release published of each upload its entries name (D-171): the version current
--    when it was released — digest, verified type, dimensions, renditions — keyed by asset id.
--    Delivery turns it into versioned, immutable URLs, so a site shows exactly the bytes that
--    were released and a replaced image reaches it at the next publish. Only usable assets
--    are stamped: a release naming anything else is refused (bad_asset) and rolls back.
--    Runs as the caller, inside the release transaction, under the project's row security.
ALTER TABLE publications ADD COLUMN media jsonb NOT NULL DEFAULT '{}'::jsonb
    CHECK (jsonb_typeof(media) = 'object');

CREATE FUNCTION colophon_release_media(project uuid, environment uuid, revision uuid)
    RETURNS jsonb
    LANGUAGE sql
    STABLE
    SET search_path = pg_catalog, pg_temp
AS $$
    SELECT coalesce(
        jsonb_object_agg(
            a.id::text,
            jsonb_build_object(
                'sha', encode(a.sha256, 'hex'),
                'type', a.detected_type,
                'width', a.width,
                'height', a.height,
                'renditions', to_jsonb(a.derivatives)
            )
        ),
        '{}'::jsonb
    )
    FROM (
        SELECT DISTINCT r.asset_id
        FROM public.asset_references r
        WHERE r.project_id = project AND r.environment_id = environment
          AND r.revision_id = revision
    ) named
    JOIN public.assets a
      ON a.project_id = project AND a.environment_id = environment AND a.id = named.asset_id
    WHERE a.state = 'usable' AND a.sha256 IS NOT NULL
$$;
REVOKE ALL ON FUNCTION colophon_release_media(uuid, uuid, uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_release_media(uuid, uuid, uuid) TO colophon_api;

-- What a release's new publication of `entry` at `revision` records: a rollback (D-097)
-- re-serves exactly the versions the release it returns to published for that revision;
-- anything else stamps the versions current now.
CREATE FUNCTION colophon_publication_media(project uuid, environment uuid, changeset uuid,
                                           entry uuid, revision uuid)
    RETURNS jsonb
    LANGUAGE sql
    STABLE
    SET search_path = pg_catalog, pg_temp
AS $$
    SELECT coalesce(
        (SELECT o.media
         FROM public.changesets c
         JOIN public.publications o
           ON o.project_id = c.project_id AND o.environment_id = c.environment_id
          AND o.entry_id = entry AND o.revision_id = revision
          AND o.valid_from <= c.rollback_generation
          AND coalesce(o.valid_to, c.rollback_generation + 1) > c.rollback_generation
         WHERE c.project_id = project AND c.environment_id = environment AND c.id = changeset
           AND c.rollback_generation IS NOT NULL
         LIMIT 1),
        public.colophon_release_media(project, environment, revision)
    )
$$;
REVOKE ALL ON FUNCTION colophon_publication_media(uuid, uuid, uuid, uuid, uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_publication_media(uuid, uuid, uuid, uuid, uuid) TO colophon_api;


-- Publications released before this migration get the version current now: an approximation
-- (a replace landed since their release is stamped as if they had published it), the best the
-- history can say — asset_versions records when each version landed, not which release saw it.
ALTER TABLE publications NO FORCE ROW LEVEL SECURITY;
ALTER TABLE asset_references NO FORCE ROW LEVEL SECURITY;
ALTER TABLE assets NO FORCE ROW LEVEL SECURITY;
UPDATE publications p
SET media = colophon_release_media(p.project_id, p.environment_id, p.revision_id)
WHERE EXISTS (
    SELECT 1 FROM asset_references r
    WHERE r.project_id = p.project_id AND r.environment_id = p.environment_id
      AND r.revision_id = p.revision_id
);
ALTER TABLE publications FORCE ROW LEVEL SECURITY;
ALTER TABLE asset_references FORCE ROW LEVEL SECURITY;
ALTER TABLE assets FORCE ROW LEVEL SECURITY;

-- 3. The library's own words for an upload (D-172): a default alt text the editor's image
--    dialog starts from, a title, a credit and up to ten slug-shaped tags, changed through
--    PATCH under If-Match on metadata_version. `search` is what `?q=` matches, served by a
--    trigram index (pg_trgm, migration 0015).
ALTER TABLE assets
    ADD COLUMN alt text CHECK (char_length(alt) BETWEEN 1 AND 300),
    ADD COLUMN title text CHECK (char_length(title) BETWEEN 1 AND 200),
    ADD COLUMN credit text CHECK (char_length(credit) BETWEEN 1 AND 200),
    ADD COLUMN tags text[] NOT NULL DEFAULT '{}' CHECK (
        cardinality(tags) <= 10
        AND array_position(tags, NULL) IS NULL
        AND array_position(tags, '') IS NULL
        AND array_to_string(tags, ' ') ~ '^([a-z0-9][a-z0-9-]{0,39}( |$))*$'
    ),
    ADD COLUMN metadata_version integer NOT NULL DEFAULT 1 CHECK (metadata_version >= 1),
    ADD COLUMN search text GENERATED ALWAYS AS (
        file_name || ' ' || coalesce(title, '') || ' ' || coalesce(alt, '')
    ) STORED;

GRANT UPDATE (alt, title, credit, tags, metadata_version) ON assets TO colophon_api;

-- The library list: newest first per environment, keyset on (created_at, id).
CREATE INDEX assets_library ON assets (project_id, environment_id, created_at DESC, id DESC);
CREATE INDEX assets_search_trgm ON assets USING gin (search gin_trgm_ops);
CREATE INDEX assets_tags ON assets USING gin (tags);

-- PATCH {P}/assets/{asset} is claimed under its own operation.
ALTER TABLE idempotency_records DROP CONSTRAINT idempotency_records_operation_check;
ALTER TABLE idempotency_records ADD CONSTRAINT idempotency_records_operation_check
    CHECK (operation IN ('entry.create', 'entry.save', 'entry.delete', 'entry.import',
                         'entry.localize',
                         'content_type.create', 'content_type.update', 'content_type.delete',
                         'changeset.create', 'changeset.publish',
                         'component.create', 'component.update',
                         'asset.upload', 'asset.replace', 'asset.delete', 'asset.update',
                         'migration.plan', 'migration.approve', 'migration.apply',
                         'migration.cancel', 'migration.resolve',
                         'comment.create', 'comment.resolve',
                         'task.create', 'task.update', 'stages.update',
                         'bulk.submit', 'bulk.items', 'bulk.cancel'));
