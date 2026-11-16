-- Milestone M, step M.5b: the worker drives a running content-migration plan to done or
-- blocked. docs/modules/02-db.md, 05-worker.md; D-101.
--
-- Applying a plan writes things the worker's own grants deliberately do not reach: a new
-- content_type_versions row (the version the run writes against), revisions with their
-- references, asset references and unique claims, and the entry's draft pointer. The API role
-- holds those grants but is confined to a project context the worker connection doesn't have,
-- so the two functions below are SECURITY DEFINER, owned by a new role `colophon_migrator`:
-- a member of colophon_api (it inherits exactly the API's table grants) whose own policies
-- let it see every project's rows, as the worker's `_for_workers` policies already do on the
-- tables it reads. Not the table owner, for the reason migration 0030 names; and only
-- colophon_worker may execute them.
--
-- Both functions take the caller's transaction as their own, like colophon_delete_entry
-- (0030): every check a run makes is under the row locks the same transaction holds, so a
-- decision can't drift between checking and writing.
--
-- A run claims a plan with a session advisory lock in Rust (`pg_try_advisory_lock`), held on
-- a dedicated connection for the whole drive: at most one worker drives a plan, and a crash
-- releases it for the next scan. These functions only ever see one driver.

-- The functions' owner. Production creates it with the other privilege roles
-- (deploy/postgres/roles.sql); this only makes sure it exists, as 0030 does for
-- colophon_deleter. Fresh test databases migrate concurrently, so losing a race to create it
-- or to grant it is fine.
DO $$
BEGIN
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'colophon_migrator') THEN
        CREATE ROLE colophon_migrator NOLOGIN NOSUPERUSER NOBYPASSRLS;
    END IF;
EXCEPTION WHEN duplicate_object OR unique_violation THEN
    NULL;
END
$$;
DO $$
BEGIN
    IF NOT pg_has_role('colophon_migrator', 'colophon_api', 'MEMBER') THEN
        GRANT colophon_api TO colophon_migrator WITH INHERIT TRUE;
    END IF;
EXCEPTION WHEN duplicate_object OR unique_violation THEN
    NULL;
END
$$;

-- What the migrator has beyond the API's own: it sees every project's rows, because the
-- worker's transaction sets no app.project_id. Grants stay the API's, inherited; the
-- policies are the row-visibility part.
CREATE POLICY migration_plans_for_migrator ON migration_plans FOR ALL TO colophon_migrator
    USING (true) WITH CHECK (true);
CREATE POLICY migration_items_for_migrator ON migration_items FOR ALL TO colophon_migrator
    USING (true) WITH CHECK (true);
CREATE POLICY entries_for_migrator ON entries FOR ALL TO colophon_migrator
    USING (true) WITH CHECK (true);
CREATE POLICY revisions_for_migrator ON revisions FOR ALL TO colophon_migrator
    USING (true) WITH CHECK (true);
CREATE POLICY revision_references_for_migrator ON revision_references FOR ALL
    TO colophon_migrator USING (true) WITH CHECK (true);
CREATE POLICY asset_references_for_migrator ON asset_references FOR ALL TO colophon_migrator
    USING (true) WITH CHECK (true);
CREATE POLICY unique_claims_for_migrator ON unique_claims FOR ALL TO colophon_migrator
    USING (true) WITH CHECK (true);
CREATE POLICY content_type_versions_for_migrator ON content_type_versions FOR ALL
    TO colophon_migrator USING (true) WITH CHECK (true);

-- A run's draft check reads the entry's current draft pointer, and the transform reads the
-- expected revision's content. Both are the worker's own reads — column grants and SELECT
-- policies like its other reads; tombstones included, so a deleted entry can be told from a
-- live one (the apply answers `skipped` for it).
CREATE POLICY revisions_for_workers ON revisions FOR SELECT TO colophon_worker USING (true);
GRANT SELECT (project_id, environment_id, entry_id, id, content) ON revisions
    TO colophon_worker;

-- The first transaction of a plan's drive: stamps the run's target version. Locks the plan,
-- then the collection's change lock (the key lock_for_change uses, so a definition write
-- queues behind or ahead of this honestly), and only then writes the base+1 version — the
-- same insert `content_types::insert` performs — and points the plan at it. One row:
-- `outcome` is `begun` (or `already` when a previous drive stamped it — a crash between
-- stamping and applying resumes exactly), `moved` (the collection's current version is no
-- longer the plan's base: the plan can't write what it was approved on), or the plan's state
-- when it isn't running (`not_running`, `superseded`, ...); `missing` when there is no plan.
CREATE FUNCTION colophon_migration_begin(project uuid, environment uuid, plan uuid)
    RETURNS TABLE (outcome text, target_version integer)
    LANGUAGE plpgsql
    VOLATILE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    stored record;
    current_version integer;
BEGIN
    -- The run's own row, held still for the transaction.
    SELECT p.state, p.collection, p.base_version, p.target_version, p.target_definition,
           p.target_definition_hash, coalesce(p.approved_by, p.created_by) AS author
      INTO stored
    FROM public.migration_plans p
    WHERE p.project_id = project AND p.environment_id = environment AND p.id = plan
    FOR UPDATE;
    IF NOT FOUND THEN
        RETURN QUERY SELECT 'missing'::text, NULL::integer;
        RETURN;
    END IF;
    IF stored.state <> 'running' THEN
        RETURN QUERY SELECT 'not_running'::text, NULL::integer;
        RETURN;
    END IF;
    IF stored.target_version IS NOT NULL THEN
        -- A drive that already stamped the run's version, resumed after a crash.
        RETURN QUERY SELECT 'already'::text, stored.target_version;
        RETURN;
    END IF;
    -- The collection's exclusive lock, the same one a type update takes (D-055): the version
    -- this reads below is the version there is until this transaction commits, and a write
    -- that holds the lock first sees `running` and refuses before it can supersede.
    PERFORM pg_advisory_xact_lock(
        17236, hashtext(project::text || '/' || environment::text || '/' || stored.collection));
    SELECT max(v.version) INTO current_version
    FROM public.content_type_versions v
    WHERE v.project_id = project AND v.environment_id = environment
      AND v.collection = stored.collection;
    IF current_version IS DISTINCT FROM stored.base_version THEN
        RETURN QUERY SELECT 'moved'::text, NULL::integer;
        RETURN;
    END IF;
    -- The version write itself: the same columns content_types::insert fills, authored by
    -- the approver whose approval let the run start.
    INSERT INTO public.content_type_versions
        (project_id, environment_id, collection, version, previous_version, definition,
         definition_hash, created_by)
    VALUES (project, environment, stored.collection, stored.base_version + 1,
            stored.base_version, stored.target_definition, stored.target_definition_hash,
            stored.author);
    UPDATE public.migration_plans p
       SET target_version = stored.base_version + 1, updated_at = now()
     WHERE p.project_id = project AND p.environment_id = environment AND p.id = plan;
    RETURN QUERY SELECT 'begun'::text, stored.base_version + 1;
END
$$;

ALTER FUNCTION colophon_migration_begin(uuid, uuid, uuid) OWNER TO colophon_migrator;
REVOKE ALL ON FUNCTION colophon_migration_begin(uuid, uuid, uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_migration_begin(uuid, uuid, uuid) TO colophon_worker;

-- One item's write: the worker transformed the expected revision's document and validated it
-- against the target; this locks the plan, the item and the entry, decides the item's fate
-- and does the whole revision write as entries.rs does — revision, references, assets,
-- unique claims, draft pointer — never a raw insert beside the side tables. One row:
-- `outcome` is `applied`, `skipped` (the entry was deleted since the plan named it),
-- `draft_moved` (the draft is no longer the expected revision: a save won the lock first, so
-- the item blocks rather than overwrite a newer edit), `missing_reference` or
-- `missing_asset` (the document names what isn't there — a person must fix it), the item's
-- terminal outcome when it already has one, `not_running` when the plan isn't, or `missing`
-- when the plan or item isn't there.
CREATE FUNCTION colophon_migration_apply_item(
    project uuid, environment uuid, plan uuid, entry uuid,
    expected uuid, revision uuid, content text, content_hash bytea,
    title text, slug text, references_json jsonb, assets_json jsonb,
    unique_fields text[])
    RETURNS TABLE (outcome text)
    LANGUAGE plpgsql
    VOLATILE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    stored record;
    stored_outcome text;
    draft uuid;
BEGIN
    -- Lock order is plan, item, entry — the only order the driver takes them, so two
    -- statements in one run can't deadlock on each other.
    SELECT p.state, p.collection, p.target_version,
           coalesce(p.approved_by, p.created_by) AS author
      INTO stored
    FROM public.migration_plans p
    WHERE p.project_id = project AND p.environment_id = environment AND p.id = plan
    FOR UPDATE;
    IF NOT FOUND THEN
        RETURN QUERY SELECT 'missing'::text;
        RETURN;
    END IF;
    IF stored.state <> 'running' OR stored.target_version IS NULL THEN
        RETURN QUERY SELECT 'not_running'::text;
        RETURN;
    END IF;
    SELECT i.outcome INTO stored_outcome
    FROM public.migration_items i
    WHERE i.project_id = project AND i.environment_id = environment
      AND i.plan_id = plan AND i.entry_id = entry
    FOR UPDATE;
    IF NOT FOUND THEN
        RETURN QUERY SELECT 'missing'::text;
        RETURN;
    END IF;
    -- A terminal item is already done: a resumed drive asks again and hears it again.
    IF stored_outcome IN ('applied', 'skipped') THEN
        RETURN QUERY SELECT stored_outcome;
        RETURN;
    END IF;
    -- The entry it names, locked so a save queues behind or ahead of this write: deleted
    -- since the plan names it (the item's row stays — the plan named it), or its draft moved.
    SELECT e.draft_revision_id INTO draft
    FROM public.entries e
    WHERE e.project_id = project AND e.environment_id = environment AND e.id = entry
      AND e.deleted_at IS NULL
    FOR UPDATE;
    IF NOT FOUND THEN
        UPDATE public.migration_items i
           SET outcome = 'skipped'
         WHERE i.project_id = project AND i.environment_id = environment
           AND i.plan_id = plan AND i.entry_id = entry;
        RETURN QUERY SELECT 'skipped'::text;
        RETURN;
    END IF;
    IF draft IS DISTINCT FROM expected THEN
        UPDATE public.migration_items i
           SET outcome = 'blocked',
               problem = jsonb_build_object(
                   'reason', 'draft_moved',
                   'pointer', '',
                   'message', 'the draft changed after the plan was made; review it and plan again')
         WHERE i.project_id = project AND i.environment_id = environment
           AND i.plan_id = plan AND i.entry_id = entry;
        RETURN QUERY SELECT 'draft_moved'::text;
        RETURN;
    END IF;
    -- What the document names must be there, checked like a save does (the foreign keys are
    -- still the rule; these name what failed, for the problem a person reads).
    IF EXISTS (
        SELECT 1 FROM jsonb_to_recordset(coalesce(references_json, '[]'::jsonb))
                  AS named (field text, position smallint, collection text, entry uuid)
        WHERE NOT EXISTS (
            SELECT 1 FROM public.entries e
            WHERE e.project_id = project AND e.environment_id = environment
              AND e.id = named.entry AND e.collection = named.collection
              AND e.deleted_at IS NULL)
    ) THEN
        UPDATE public.migration_items i
           SET outcome = 'blocked',
               problem = jsonb_build_object(
                   'reason', 'missing_reference',
                   'pointer', '',
                   'message', 'a reference names an entry that is not there; fix it and plan again')
         WHERE i.project_id = project AND i.environment_id = environment
           AND i.plan_id = plan AND i.entry_id = entry;
        RETURN QUERY SELECT 'missing_reference'::text;
        RETURN;
    END IF;
    IF EXISTS (
        SELECT 1 FROM jsonb_to_recordset(coalesce(assets_json, '[]'::jsonb))
                  AS named (field text, position smallint, asset uuid)
        WHERE NOT EXISTS (
            SELECT 1 FROM public.assets a
            WHERE a.project_id = project AND a.environment_id = environment
              AND a.id = named.asset)
    ) THEN
        UPDATE public.migration_items i
           SET outcome = 'blocked',
               problem = jsonb_build_object(
                   'reason', 'missing_asset',
                   'pointer', '',
                   'message', 'an asset field names an upload that is not there; fix it and plan again')
         WHERE i.project_id = project AND i.environment_id = environment
           AND i.plan_id = plan AND i.entry_id = entry;
        RETURN QUERY SELECT 'missing_asset'::text;
        RETURN;
    END IF;
    -- The revision write, the same rows entries.rs keeps for a save: the revision built on
    -- the expected one and authored by the approver, then its references and assets, the new
    -- draft pointer, its draft unique claims, and the item that records it.
    INSERT INTO public.revisions
        (project_id, environment_id, entry_id, id, base_revision_id, content, content_hash,
         title, author_id, collection, schema_version, slug)
    VALUES (project, environment, entry, revision, expected, content, content_hash,
            title, stored.author, stored.collection, stored.target_version, slug);
    INSERT INTO public.revision_references
        (project_id, environment_id, revision_id, field, position, collection, entry_id)
    SELECT project, environment, revision, named.field, named.position, named.collection,
           named.entry
    FROM jsonb_to_recordset(coalesce(references_json, '[]'::jsonb))
         AS named (field text, position smallint, collection text, entry uuid);
    INSERT INTO public.asset_references
        (project_id, environment_id, revision_id, field, position, asset_id)
    SELECT project, environment, revision, named.field, named.position, named.asset
    FROM jsonb_to_recordset(coalesce(assets_json, '[]'::jsonb))
         AS named (field text, position smallint, asset uuid);
    UPDATE public.entries e
       SET draft_revision_id = revision, draft_slug = slug, draft_title = title,
           updated_at = now()
     WHERE e.project_id = project AND e.environment_id = environment AND e.id = entry;
    IF cardinality(unique_fields) > 0 THEN
        DELETE FROM public.unique_claims c
        WHERE c.project_id = project AND c.environment_id = environment
          AND c.entry_id = entry AND NOT c.published;
        INSERT INTO public.unique_claims
            (project_id, environment_id, collection, field, published, value_hash, entry_id)
        SELECT project, environment, stored.collection, v.field, false, v.value_hash, entry
        FROM public.colophon_unique_values(unique_fields, content) AS v;
    END IF;
    UPDATE public.migration_items i
       SET outcome = 'applied', new_revision_id = revision, applied_at = now()
     WHERE i.project_id = project AND i.environment_id = environment
       AND i.plan_id = plan AND i.entry_id = entry;
    RETURN QUERY SELECT 'applied'::text;
END
$$;

ALTER FUNCTION colophon_migration_apply_item(
    uuid, uuid, uuid, uuid, uuid, uuid, text, bytea, text, text, jsonb, jsonb, text[])
    OWNER TO colophon_migrator;
REVOKE ALL ON FUNCTION colophon_migration_apply_item(
    uuid, uuid, uuid, uuid, uuid, uuid, text, bytea, text, text, jsonb, jsonb, text[])
    FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_migration_apply_item(
    uuid, uuid, uuid, uuid, uuid, uuid, text, bytea, text, text, jsonb, jsonb, text[])
    TO colophon_worker;
