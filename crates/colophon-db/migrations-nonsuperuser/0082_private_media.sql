-- NON-SUPERUSER SUBSTITUTE (D-234). Run instead of migrations/0082_private_media.sql only when the migrating role is not a
-- superuser (Neon's owner role, for one): PostgreSQL lets only a superuser, or a role granted SET on the parameter,
-- attach `SET app.project_id = ''` (an undeclared, custom setting) to a function, and Neon allows neither. This file
-- is the original byte for byte except that each such clause is replaced by the same guarantee written in the body:
-- the caller's value is captured on entry, the function sets its own, and every normal return path puts the
-- caller's value back. An error needs nothing: a value set with set_config(..., true) is rolled back with the
-- transaction, or with the savepoint that caught the error, exactly as the clause's would be. An unset value comes
-- back as '' (every reader takes nullif(..., '')). The database records the ORIGINAL file's checksum
-- (colophon_db::nonsuperuser), so it looks the same to sqlx and `admin migrate-status` either way.
--
-- D-229: media is private until it is published.
--
-- The media origin served any usable upload to anyone who knew its project, environment and asset id. Asset ids are
-- UUIDv7, whose leading 48 bits are a timestamp, and the remaining random bits are not an access control. Now
-- `media_object` also says whether the version it would serve is published: named by the `media` (what a release
-- stamped, D-178) of a publication the public may read and no takedown revoked — any generation's, since delivery
-- still serves a site pinned to an older release (`?generation=`) the media that release named. The origin serves a published version publicly as
-- before, and anything else only to a signed-in member of the project or with a short-lived signed address
-- (preview answers and members-only delivery sign theirs), privately. The bare address of an asset whose newest
-- version waits for its publish shows the public the newest published one, as releases promised (D-178: a replaced
-- image reaches a site at the next publish). To everyone else it is a 404, exactly like an unknown asset, so the
-- answer never confirms an upload exists.
--
-- `published` runs as `media_object`'s owner, the delivery role, whose row security lets through only `public`
-- publications when no audience is set: an image named only by members-only content is not public.

-- Which publications name an asset: `media ? asset` reaches only the rows that name it, in any project.
CREATE INDEX publications_media_named ON publications USING gin (media jsonb_ops)
    WHERE revoked_in IS NULL;

-- The bare address picks the newest published version by its landing order, which the delivery role (the function's
-- owner) could not read until now; it still reads nothing else of a version.
GRANT SELECT (seq) ON asset_versions TO colophon_delivery;

DROP FUNCTION media_object(uuid, text, uuid, text);

CREATE FUNCTION media_object(project uuid, environment_slug text, asset uuid,
                             version_sha text DEFAULT NULL)
    RETURNS TABLE (environment_id uuid, public_key text, published boolean, published_key text)
    LANGUAGE plpgsql
    VOLATILE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    caller_project_id text := current_setting('app.project_id', true);
    caller_audience_role text := current_setting('app.audience_role', true);
BEGIN
    PERFORM set_config('app.audience_role', '', true);
    PERFORM set_config('app.project_id', project::text, true);
    RETURN QUERY
        WITH served AS (
            SELECT e.id AS environment_id,
                   CASE WHEN version_sha IS NULL THEN a.public_key ELSE v.public_key END AS public_key
            FROM public.environments e
            LEFT JOIN public.assets a
                   ON a.project_id = e.project_id AND a.environment_id = e.id AND a.id = asset
            LEFT JOIN public.asset_versions v
                   ON version_sha IS NOT NULL
                  AND v.project_id = a.project_id AND v.environment_id = a.environment_id
                  AND v.asset_id = a.id AND encode(v.sha256, 'hex') = version_sha
            WHERE e.project_id = project AND e.slug = environment_slug
        ),
        -- The asset's versions a public publication names that no takedown revoked: what delivery can
        -- still serve at some generation, a pinned build's included.
        live AS (
            SELECT cv.public_key, cv.seq
            FROM public.asset_versions cv
            JOIN served s ON cv.environment_id = s.environment_id
            WHERE cv.project_id = project AND cv.asset_id = asset
              AND EXISTS (
                  SELECT 1 FROM public.publications p
                  WHERE p.project_id = cv.project_id AND p.environment_id = cv.environment_id
                    AND p.revoked_in IS NULL
                    AND p.media ? asset::text
                    AND p.media -> asset::text ->> 'sha' = encode(cv.sha256, 'hex')
              )
        )
        SELECT s.environment_id,
               s.public_key,
               s.public_key IS NOT NULL
                   AND EXISTS (SELECT 1 FROM live l WHERE l.public_key = s.public_key),
               -- What the bare address shows the public while a replace waits for its publish: the
               -- newest published version of a usable asset (none for a pinned address).
               CASE WHEN version_sha IS NULL AND s.public_key IS NOT NULL
                    THEN (SELECT l.public_key FROM live l ORDER BY l.seq DESC LIMIT 1) END
        FROM served s;
    PERFORM set_config('app.project_id', coalesce(caller_project_id, ''), true);
    PERFORM set_config('app.audience_role', coalesce(caller_audience_role, ''), true);
END
$$;
COMMENT ON FUNCTION media_object(uuid, text, uuid, text) IS 'D-234 non-superuser substitute: the caller''s app.* settings are captured and restored in the body instead of a SET clause (colophon_db::nonsuperuser)';

ALTER FUNCTION media_object(uuid, text, uuid, text) OWNER TO colophon_delivery;
REVOKE ALL ON FUNCTION media_object(uuid, text, uuid, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION media_object(uuid, text, uuid, text) TO colophon_api;

-- The key that signs short-lived media addresses (D-229): one per installation, made here, read by every server, so
-- an address one server signs another verifies. Only the API reads it; nothing writes it after this migration.
CREATE TABLE media_signing_keys (
    id smallint PRIMARY KEY CHECK (id = 1),
    key bytea NOT NULL CHECK (length(key) = 32),
    created_at timestamptz NOT NULL DEFAULT now()
);
-- 32 bytes from two version-4 UUIDs' random bits (244 of them): gen_random_uuid draws from the server's
-- cryptographic random source, and needs no extension.
INSERT INTO media_signing_keys (id, key)
VALUES (1, decode(replace(gen_random_uuid()::text || gen_random_uuid()::text, '-', ''), 'hex'));
GRANT SELECT ON media_signing_keys TO colophon_api;
