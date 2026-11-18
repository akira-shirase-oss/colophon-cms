-- Locale selection in delivery (milestone E; D-181, D-182). docs/modules/02-db.md,
-- docs/modules/06-delivery.md ("Locales").
--
-- D-087 gave entries a locale tag and translation groups, but publications carried neither, so
-- `/content/v1` could only hand readers every language at once. This migration does for the
-- locale what 0052 did for visibility: the revision pins it, a release copies it into the
-- projection, and delivery reads it from there — never from the moving draft.
--
-- 1. `revisions.locale`: the tag the entry had when the revision was written. A trigger sets
--    it from the entry on every insert, so every writer — saves, creates, localizations, bulk
--    items and the content-migration run (0041), which writes revisions in SQL — pins the
--    same thing without each having to remember to. A save that changes the tag moves the
--    entry first and writes its revision after, in the same transaction. Revisions written
--    before this migration are backfilled from their entry's current tag: an approximation
--    for history (a tag changed since is not recovered), stated in module 02.
-- 2. `publications.locale` and `publications.group_key`: copied when a release opens a
--    publication — the locale from the revision, the group from the entry: its
--    `localization_id`, or the entry's own id when it has none. A group never changes once
--    set (0034's CHECK), and from now on a new group takes its source entry's id, so a
--    source published before it was translated keeps the same key afterwards. Existing groups
--    are backfilled from `entries` here, which is exact.
-- 3. `environment_locales`: an environment's languages — at most 50, one default, each with
--    up to four ordered fallbacks drawn from the list. `environments.locale_policy_version`
--    counts its changes; delivery names it in cache keys, so a changed policy never serves a
--    resolution cached under the old one.
-- 4. `delivery_environment(project, slug, requested)`: the one statement delivery runs before
--    asking its cache now also answers the policy: its version, the listed tags (at most 50),
--    the default tag, and the requested tag's listed spelling and fallback chain.

-- The same shape the API checks (`colophon_domain::locale::check_tag`): a 2–3 letter primary
-- subtag, then subtags of 1–8 letters or digits, 35 characters in all.
ALTER TABLE revisions ADD COLUMN locale text CHECK (
    locale IS NULL
    OR (char_length(locale) <= 35 AND locale ~ '^[A-Za-z]{2,3}(-[A-Za-z0-9]{1,8})*$'));
ALTER TABLE publications ADD COLUMN locale text CHECK (
    locale IS NULL
    OR (char_length(locale) <= 35 AND locale ~ '^[A-Za-z]{2,3}(-[A-Za-z0-9]{1,8})*$'));
ALTER TABLE publications ADD COLUMN group_key uuid;

-- The backfill reads across projects as the tables' owner: forced row security would show it
-- nothing, so it is lifted for these statements only (as 0052 does for roles).
ALTER TABLE entries NO FORCE ROW LEVEL SECURITY;
ALTER TABLE revisions NO FORCE ROW LEVEL SECURITY;
ALTER TABLE publications NO FORCE ROW LEVEL SECURITY;
UPDATE revisions r SET locale = e.locale
FROM entries e
WHERE e.project_id = r.project_id AND e.environment_id = r.environment_id AND e.id = r.entry_id
  AND e.locale IS NOT NULL;
UPDATE publications p SET locale = r.locale
FROM revisions r
WHERE r.project_id = p.project_id AND r.environment_id = p.environment_id AND r.id = p.revision_id
  AND r.locale IS NOT NULL;
UPDATE publications p SET group_key = coalesce(e.localization_id, p.entry_id)
FROM entries e
WHERE e.project_id = p.project_id AND e.environment_id = p.environment_id AND e.id = p.entry_id;
-- A publication whose entry row is gone entirely (none should be: entries are tombstoned, not
-- deleted) is its own group.
UPDATE publications SET group_key = entry_id WHERE group_key IS NULL;
ALTER TABLE entries FORCE ROW LEVEL SECURITY;
ALTER TABLE revisions FORCE ROW LEVEL SECURITY;
ALTER TABLE publications FORCE ROW LEVEL SECURITY;
ALTER TABLE publications ALTER COLUMN group_key SET NOT NULL;

-- Delivery's locale reads (module 06 has their plans): a page in one language reads each tag
-- of its chain straight off its own range, in group order; an entry's translations are its
-- group's rows. Tags compare exactly here: under `colophon_delivery`'s row security only
-- leakproof comparisons reach an index, and `lower()` isn't one. The list's spelling is what
-- writes store and what requests are respelled to.
CREATE INDEX publications_by_group
    ON publications (project_id, environment_id, collection, group_key DESC);
CREATE INDEX publications_by_locale
    ON publications (project_id, environment_id, collection, locale, group_key DESC);

-- Every revision pins its entry's tag as it is written.
CREATE FUNCTION colophon_revision_locale() RETURNS trigger
    LANGUAGE plpgsql
    SET search_path = pg_catalog, public
AS $$
BEGIN
    NEW.locale := (SELECT e.locale FROM public.entries e
                   WHERE e.project_id = NEW.project_id AND e.environment_id = NEW.environment_id
                     AND e.id = NEW.entry_id);
    RETURN NEW;
END
$$;
CREATE TRIGGER revisions_pin_locale BEFORE INSERT ON revisions
    FOR EACH ROW EXECUTE FUNCTION colophon_revision_locale();

-- An environment's languages, in display order.
CREATE TABLE environment_locales (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    tag text NOT NULL CHECK (
        char_length(tag) <= 35 AND tag ~ '^[A-Za-z]{2,3}(-[A-Za-z0-9]{1,8})*$'),
    label text NOT NULL CHECK (char_length(label) BETWEEN 1 AND 80),
    position smallint NOT NULL CHECK (position BETWEEN 0 AND 49),
    is_default boolean NOT NULL DEFAULT false,
    -- Tried in order when the tag itself isn't published: listed tags, never the tag itself.
    -- That every one is listed is the API's rule (`fallback_not_listed`), checked with the
    -- whole list in hand.
    fallback text[] NOT NULL DEFAULT '{}' CHECK (
        cardinality(fallback) <= 4 AND NOT (tag = ANY (fallback))),
    PRIMARY KEY (project_id, environment_id, tag),
    UNIQUE (project_id, environment_id, position),
    FOREIGN KEY (project_id, environment_id) REFERENCES environments (project_id, id)
        ON DELETE CASCADE
);
-- One default per environment. "At least one" is the API's rule: a list is replaced whole.
CREATE UNIQUE INDEX environment_locales_one_default
    ON environment_locales (project_id, environment_id) WHERE is_default;
-- One spelling per tag: `pt-br` is `pt-BR`, as BCP 47 has it, so a list can't hold both.
CREATE UNIQUE INDEX environment_locales_one_spelling
    ON environment_locales (project_id, environment_id, lower(tag));

ALTER TABLE environment_locales ENABLE ROW LEVEL SECURITY;
ALTER TABLE environment_locales FORCE ROW LEVEL SECURITY;
CREATE POLICY environment_locales_in_project ON environment_locales FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
CREATE POLICY environment_locales_delivered ON environment_locales FOR SELECT
    TO colophon_delivery
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
GRANT SELECT, INSERT, DELETE ON environment_locales TO colophon_api;
GRANT SELECT ON environment_locales TO colophon_delivery;

ALTER TABLE environments ADD COLUMN locale_policy_version bigint NOT NULL DEFAULT 0
    CHECK (locale_policy_version >= 0);
GRANT UPDATE (locale_policy_version) ON environments TO colophon_api;
GRANT SELECT (locale_policy_version) ON environments TO colophon_delivery;

-- Replacing the list is idempotent like the other whole-list writes: the operations 0066 last
-- listed, and `locales.update`.
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
                         'task.create', 'task.update',
                         'stages.update',
                         'bulk.submit', 'bulk.items', 'bulk.cancel',
                         'locales.update'));

-- The per-request lookup grows the locale policy. A new result type means a new function
-- (CREATE OR REPLACE can't change it); otherwise the rules of 0011 and 0012: it runs as
-- colophon_delivery inside the requested project's context, which its own SET clause keeps
-- from outliving the call. `requested` is matched without case and answered in the list's
-- spelling, so `pt-br` and `pt-BR` share one cache key.
DROP FUNCTION delivery_environment(uuid, text);
CREATE FUNCTION delivery_environment(project uuid, environment_slug text, requested text)
    RETURNS TABLE (id uuid, epoch bigint, generation bigint, last_unpublish bigint,
                   locale_policy bigint, locales text[], default_locale text,
                   listed_locale text, chain text[])
    LANGUAGE plpgsql
    VOLATILE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
    SET app.project_id = ''
AS $$
BEGIN
    PERFORM set_config('app.project_id', project::text, true);
    RETURN QUERY
        SELECT e.id, e.epoch, e.generation, e.last_unpublish, e.locale_policy_version,
               coalesce((SELECT array_agg(l.tag ORDER BY l.position)
                         FROM public.environment_locales l
                         WHERE l.project_id = e.project_id AND l.environment_id = e.id), '{}'),
               (SELECT l.tag FROM public.environment_locales l
                WHERE l.project_id = e.project_id AND l.environment_id = e.id AND l.is_default),
               r.tag, r.fallback
        FROM public.environments e
        LEFT JOIN LATERAL (
            SELECT l.tag, l.fallback FROM public.environment_locales l
            WHERE l.project_id = e.project_id AND l.environment_id = e.id
              AND lower(l.tag) = lower(requested)
        ) r ON true
        WHERE e.project_id = project AND e.slug = environment_slug;
END
$$;

ALTER FUNCTION delivery_environment(uuid, text, text) OWNER TO colophon_delivery;
REVOKE ALL ON FUNCTION delivery_environment(uuid, text, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION delivery_environment(uuid, text, text) TO colophon_api;
