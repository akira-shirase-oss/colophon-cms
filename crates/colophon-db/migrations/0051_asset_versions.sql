-- Milestone P6, media retention: object-store asset versioning. docs/modules/07-media.md,
-- D-136.
--
-- An asset's verified bytes get a version of their own, addressed by content. `asset_versions`
-- records every landing verify attempt: the version's `public_key` is the literal object key
-- (`public/{project}/{env}/{asset}/{sha256-hex}` for new writes; pre-existing assets keep their
-- one `public/{project}/{env}/{asset}` key as seq 1), and renditions sit under it. The asset
-- row's `public_key`/`sha256`/`detected_type`/dimensions/`derivatives` stay the *current*
-- version's columns — the serve path resolves them through `media_object` — while the version
-- rows are the immutable history a delete enumerates and a pinned URL (`/media/v1/…/{asset}/
-- {sha}`) names. Two versions of one asset with identical bytes coalesce on (asset, sha256):
-- the content is already stored under that key, so a second landing records no new row.
--
-- The `reverify` job kind is a replace: `POST {P}/assets/{asset}/replace` writes fresh bytes to
-- a random quarantine key (recorded on the job, so the asset's own `quarantine_key` stays the
-- first upload's), then the same verify attempt promotes them to a new content-addressed key
-- and swings the asset's current-version columns in the one transaction a delete's recheck
-- locks first. A replace asked of a quarantined asset is refused (`asset_quarantined`); a
-- failed reverify verdict leaves the current version serving; a landing that finds the asset
-- no longer `usable` records the job `superseded` and the worker removes the objects it wrote.
--
-- `media_object` is the media origin's whole read, as `delivery_environment` is delivery's:
-- SECURITY DEFINER, owned by `colophon_delivery`, the project bound inside. Given a version
-- sha it returns that version's key; without one, the usable asset's current key. The
-- `assets_delivered` policy is what keeps quarantined, rejected, deleting and deleted rows —
-- and so their objects — out of the origin's sight even while an object-store delete is still
-- in flight.

CREATE TABLE asset_versions (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    asset_id uuid NOT NULL,
    -- 1 for the first verified bytes; each distinct later landing takes max+1.
    seq integer NOT NULL CHECK (seq >= 1),
    -- The replacement's claimed MIME type and byte count, as the upload declared them (the
    -- signature still decides what the bytes are); carried on the job until landing.
    declared_type text NOT NULL CHECK (char_length(declared_type) <= 100),
    size_bytes bigint NOT NULL CHECK (size_bytes BETWEEN 1 AND 20971520),
    detected_type text NOT NULL CHECK (char_length(detected_type) <= 100),
    sha256 bytea NOT NULL,
    width integer NOT NULL CHECK (width > 0),
    height integer NOT NULL CHECK (height > 0),
    -- The literal object key this version's original lives at; renditions sit under it.
    public_key text NOT NULL UNIQUE,
    derivatives text[] NOT NULL DEFAULT '{}',
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, environment_id, asset_id, seq),
    UNIQUE (project_id, environment_id, asset_id, sha256),
    FOREIGN KEY (project_id, environment_id, asset_id)
        REFERENCES assets (project_id, environment_id, id)
);

-- Every existing usable-or-gone asset is its own seq 1: the key it was promoted to stays the
-- literal key, so a pinned URL built from its sha resolves through the same table.
INSERT INTO asset_versions (project_id, environment_id, asset_id, seq, declared_type,
                            size_bytes, detected_type, sha256, width, height, public_key,
                            derivatives, created_at)
    SELECT project_id, environment_id, id, 1, declared_type, size_bytes, detected_type,
           sha256, width, height, public_key, derivatives, coalesce(verified_at, created_at)
    FROM assets
    WHERE public_key IS NOT NULL;

-- A replace's verify job carries what the asset row can't: the fresh quarantine key and the
-- upload's declared type and size (swung onto the asset at landing, and onto its version row).
ALTER TABLE asset_jobs ADD COLUMN quarantine_key text;
ALTER TABLE asset_jobs ADD COLUMN declared_type text CHECK (char_length(declared_type) <= 100);
ALTER TABLE asset_jobs ADD COLUMN size_bytes bigint CHECK (size_bytes BETWEEN 1 AND 20971520);
ALTER TABLE asset_jobs DROP CONSTRAINT asset_jobs_kind_check;
ALTER TABLE asset_jobs ADD CONSTRAINT asset_jobs_kind_check
    CHECK (kind IN ('verify', 'reverify', 'expire', 'delete'));

ALTER TABLE asset_versions ENABLE ROW LEVEL SECURITY;
ALTER TABLE asset_versions FORCE ROW LEVEL SECURITY;
CREATE POLICY asset_versions_in_project ON asset_versions FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
CREATE POLICY asset_versions_for_workers ON asset_versions FOR ALL TO colophon_worker
    USING (true) WITH CHECK (true);

GRANT SELECT ON asset_versions TO colophon_api;
GRANT SELECT, INSERT ON asset_versions TO colophon_worker;
-- A replacement's declared type and size swing onto the asset at landing, beside the verdict
-- columns the worker already owns. The label (file_name) does not move: a replace changes
-- bytes, not what the record is called.
GRANT UPDATE (declared_type, size_bytes) ON assets TO colophon_worker;

-- The delivery role sees only usable assets' serving columns — everything else is 404 — and
-- their versions' keys; `media_object` runs with those rights, never the caller's.
CREATE POLICY assets_delivered ON assets FOR SELECT TO colophon_delivery
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid
           AND state = 'usable');
GRANT SELECT (project_id, environment_id, id, state, public_key) ON assets TO colophon_delivery;
CREATE POLICY asset_versions_delivered ON asset_versions FOR SELECT TO colophon_delivery
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
GRANT SELECT (project_id, environment_id, asset_id, sha256, public_key)
    ON asset_versions TO colophon_delivery;

-- The media origin's one read: resolve the environment slug and the key to serve in a single
-- call — the usable asset's current `public_key`, or the version `version_sha` names (its
-- literal stored key, so any object layout answers). Nothing for an unknown environment, an
-- asset delivery may not see, or a version that isn't this asset's.
CREATE FUNCTION media_object(project uuid, environment_slug text, asset uuid,
                             version_sha text DEFAULT NULL)
    RETURNS TABLE (environment_id uuid, public_key text)
    LANGUAGE plpgsql
    VOLATILE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
    SET app.project_id = ''
AS $$
BEGIN
    PERFORM set_config('app.project_id', project::text, true);
    RETURN QUERY
        SELECT e.id,
               CASE WHEN version_sha IS NULL THEN a.public_key ELSE v.public_key END
        FROM public.environments e
        LEFT JOIN public.assets a
               ON a.project_id = e.project_id AND a.environment_id = e.id AND a.id = asset
        LEFT JOIN public.asset_versions v
               ON version_sha IS NOT NULL
              AND v.project_id = a.project_id AND v.environment_id = a.environment_id
              AND v.asset_id = a.id AND encode(v.sha256, 'hex') = version_sha
        WHERE e.project_id = project AND e.slug = environment_slug;
END
$$;

ALTER FUNCTION media_object(uuid, text, uuid, text) OWNER TO colophon_delivery;
REVOKE ALL ON FUNCTION media_object(uuid, text, uuid, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION media_object(uuid, text, uuid, text) TO colophon_api;

-- The replace request claims its idempotency key like every other write.
ALTER TABLE idempotency_records DROP CONSTRAINT idempotency_records_operation_check;
ALTER TABLE idempotency_records ADD CONSTRAINT idempotency_records_operation_check
    CHECK (operation IN ('entry.create', 'entry.save', 'entry.delete', 'entry.import',
                         'entry.localize',
                         'content_type.create', 'content_type.update', 'content_type.delete',
                         'changeset.create', 'changeset.publish',
                         'component.create', 'component.update',
                         'asset.upload', 'asset.replace', 'asset.delete',
                         'migration.plan', 'migration.approve', 'migration.apply',
                         'migration.cancel', 'migration.resolve',
                         'comment.create', 'comment.resolve',
                         'task.create', 'task.update', 'stages.update',
                         'bulk.submit', 'bulk.items', 'bulk.cancel'));
