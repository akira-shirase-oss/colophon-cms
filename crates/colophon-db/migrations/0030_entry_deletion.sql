-- Milestone M follow-up: deleting an entry. docs/modules/02-db.md ("Deleting entries"),
-- 04-api.md, D-080.
--
-- Deleting leaves a tombstone; no row is ever removed. The entry gets `deleted_at` and
-- `deleted_by` (a person, or an API token's own identity, as D-063 attributes), and its
-- revisions, its history and every release that published it stay exactly as they were, so every
-- foreign key that names it stays valid. From then on the application's role can't see it:
-- row-level security hides it from every query, so no list, read, history or save needs a filter
-- of its own. What it held in its collection comes free: its slug and its single type's one place
-- (both indexes below skip deleted entries) and its unique values (its claims are deleted).
--
-- Hiding a row from SELECT also stops the application's role from marking it: PostgreSQL checks
-- an UPDATE's new row against the SELECT policies as well, and a tombstoned row fails them
-- (verified on PostgreSQL 18, with or without RETURNING). So the one way to delete is the function
-- colophon_delete_entry below, SECURITY DEFINER and owned by colophon_deleter: a role with exactly
-- the API's privileges (it is a member of colophon_api) plus a policy that lets it see entries
-- and tombstone live ones. Not the table owner: production's owner isn't a superuser, entries
-- forces row-level security on its owner, and no policy names the owner, so an owner-owned
-- function would see no rows at all.
--
-- The function checks, under a row lock on the entry and each in its own statement (so each sees
-- everything committed before the lock was granted), that the draft is still the one the caller
-- looked at, that the entry isn't published, that no live entry's draft and no publication names
-- it, and that no changeset that hasn't been published pins it; then it writes the tombstone, frees
-- the unique values and appends the audit event, all in the caller's transaction. Whether the
-- caller may delete this entry is decided before, by the API, with the one policy evaluator
-- (D-058); the function enforces that it runs in the caller's own project context and records
-- the transaction's identity as the one who deleted it.
--
-- Races are decided here, not by the API's reads. The deletion locks the entry FOR UPDATE before
-- it checks anything. Inserting a reference, a changeset item or a publication locks the entry it
-- names FOR KEY SHARE (the trigger below), and the two locks conflict, so one waits for the
-- other: a reference, pin or publication that commits first is seen by the deletion's checks, and
-- one that waited finds the entry deleted (a row-locking read rechecks the newest version of the
-- row, row-level security included) and fails. The deletion's own UPDATE would not do: an UPDATE
-- that changes no key column takes FOR NO KEY UPDATE, which FOR KEY SHARE does not wait for
-- (verified on PostgreSQL 18).

-- The function's owner. Production creates it with the other privilege roles
-- (deploy/postgres/roles.sql); this only makes sure it exists, as migration 0001 does for
-- colophon_api. Fresh test databases migrate concurrently, so losing a race to create it, or to
-- grant it, is fine.
DO $$
BEGIN
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'colophon_deleter') THEN
        CREATE ROLE colophon_deleter NOLOGIN NOSUPERUSER NOBYPASSRLS;
    END IF;
EXCEPTION WHEN duplicate_object OR unique_violation THEN
    NULL;
END
$$;
DO $$
BEGIN
    IF NOT pg_has_role('colophon_deleter', 'colophon_api', 'MEMBER') THEN
        GRANT colophon_api TO colophon_deleter WITH INHERIT TRUE;
    END IF;
EXCEPTION WHEN duplicate_object OR unique_violation THEN
    NULL;
END
$$;

ALTER TABLE entries
    ADD COLUMN deleted_at timestamptz,
    ADD COLUMN deleted_by uuid REFERENCES identities (id),
    ADD CONSTRAINT entries_deleted_by_someone CHECK ((deleted_at IS NULL) = (deleted_by IS NULL));

-- A deleted entry's slug and single-type place are free: the rules count live entries only.
-- `draft_slug` keeps the draft revision's slug (D-054's invariant holds for every entry).
DROP INDEX entries_draft_slug;
CREATE UNIQUE INDEX entries_draft_slug ON entries (project_id, environment_id, collection, draft_slug)
    WHERE draft_slug IS NOT NULL AND deleted_at IS NULL;
DROP INDEX entries_single;
CREATE UNIQUE INDEX entries_single ON entries (project_id, environment_id, collection)
    WHERE single AND deleted_at IS NULL;

-- The application's role sees and changes live entries only, and can't insert a deleted one.
-- Foreign keys are checked without row-level security, so references, revisions, changeset items
-- and publications that name a deleted entry stay valid.
DROP POLICY entries_in_project ON entries;
CREATE POLICY entries_live ON entries FOR SELECT TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid
           AND deleted_at IS NULL);
CREATE POLICY entries_created ON entries FOR INSERT TO colophon_api
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid
                AND deleted_at IS NULL AND deleted_by IS NULL);
CREATE POLICY entries_changed ON entries FOR UPDATE TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid
           AND deleted_at IS NULL)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid
                AND deleted_at IS NULL);
-- What colophon_deleter has beyond the API's own: it sees the project's deleted entries too, and
-- may turn a live one into a tombstone (only it may write the two columns).
CREATE POLICY entries_deleter_sees ON entries FOR SELECT TO colophon_deleter
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
CREATE POLICY entries_deleter_tombstones ON entries FOR UPDATE TO colophon_deleter
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid
           AND deleted_at IS NULL)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid
                AND deleted_at IS NOT NULL);
GRANT UPDATE (deleted_at, deleted_by) ON entries TO colophon_deleter;

-- "Is this entry pinned by a changeset?" for each deletion; the primary key leads with the
-- changeset.
CREATE INDEX changeset_items_by_entry ON changeset_items (project_id, environment_id, entry_id);

-- Nothing new may name a deleted entry: not a reference (D-065), not a changeset pin, not a
-- publication. The entry is locked FOR KEY SHARE, which waits for a deletion holding it FOR
-- UPDATE and then rechecks the row as it is now. Runs as the caller, so for the application's
-- role row-level security hides deleted entries anyway; the explicit test covers anyone else.
-- The error is a foreign-key violation naming `<table>_entry_live`, with the entry's id as its
-- detail, so the API answers as it would for an entry that doesn't exist.
CREATE FUNCTION colophon_entry_live() RETURNS trigger
    LANGUAGE plpgsql
    SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    PERFORM 1 FROM public.entries e
    WHERE e.project_id = NEW.project_id AND e.environment_id = NEW.environment_id
      AND e.id = NEW.entry_id AND e.deleted_at IS NULL
    FOR KEY SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'entry % is deleted or does not exist', NEW.entry_id
            USING ERRCODE = 'foreign_key_violation',
                  CONSTRAINT = tg_table_name || '_entry_live',
                  TABLE = tg_table_name,
                  DETAIL = NEW.entry_id::text;
    END IF;
    RETURN NEW;
END
$$;
REVOKE ALL ON FUNCTION colophon_entry_live() FROM PUBLIC;

CREATE TRIGGER revision_references_entry_live BEFORE INSERT ON revision_references
    FOR EACH ROW EXECUTE FUNCTION colophon_entry_live();
CREATE TRIGGER changeset_items_entry_live BEFORE INSERT OR UPDATE OF revision_id ON changeset_items
    FOR EACH ROW EXECUTE FUNCTION colophon_entry_live();
CREATE TRIGGER publications_entry_live BEFORE INSERT ON publications
    FOR EACH ROW EXECUTE FUNCTION colophon_entry_live();

-- Deletes `entry` of `collection` if its draft is still `expected`, nothing depends on it, and the
-- caller's transaction is in `project`'s context (the API checked membership and permission
-- first). `event` is the id of its audit event. One row: `outcome` is `deleted`, or why not:
-- `not_found` (no such live entry here), `stale` (`draft_revision` is the draft now),
-- `published`, `referenced` or `in_changeset`. Nothing is written unless it is `deleted`.
CREATE FUNCTION colophon_delete_entry(
    project uuid, environment uuid, entry uuid, collection text, expected uuid, event uuid)
    RETURNS TABLE (outcome text, draft_revision uuid, deleted timestamptz)
    LANGUAGE plpgsql
    VOLATILE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    actor uuid := nullif(current_setting('app.identity_id', true), '')::uuid;
    draft uuid;
    title text;
BEGIN
    IF actor IS NULL
       OR project IS DISTINCT FROM nullif(current_setting('app.project_id', true), '')::uuid THEN
        RAISE EXCEPTION 'colophon_delete_entry runs only inside its own project''s context'
            USING ERRCODE = 'insufficient_privilege';
    END IF;
    -- Saves, deletions and new references, pins and publications of this entry queue here.
    SELECT e.draft_revision_id, e.draft_title INTO draft, title
    FROM public.entries e
    WHERE e.project_id = project AND e.environment_id = environment AND e.id = entry
      AND e.collection = colophon_delete_entry.collection AND e.deleted_at IS NULL
    FOR UPDATE;
    IF NOT FOUND THEN
        RETURN QUERY SELECT 'not_found'::text, NULL::uuid, NULL::timestamptz;
        RETURN;
    END IF;
    IF draft IS DISTINCT FROM expected THEN
        RETURN QUERY SELECT 'stale'::text, draft, NULL::timestamptz;
        RETURN;
    END IF;
    -- Readers get it: it must be taken down (a reviewed unpublish, D-049) first.
    IF EXISTS (
        SELECT 1 FROM public.publications p
        WHERE p.project_id = project AND p.environment_id = environment AND p.entry_id = entry
          AND p.valid_to IS NULL
    ) THEN
        RETURN QUERY SELECT 'published'::text, draft, NULL::timestamptz;
        RETURN;
    END IF;
    -- Another live entry's draft, or anything published, still names it (D-065).
    IF EXISTS (
        SELECT 1 FROM public.revision_references r
        WHERE r.project_id = project AND r.environment_id = environment AND r.entry_id = entry
          AND (EXISTS (SELECT 1 FROM public.entries e
                       WHERE e.project_id = r.project_id AND e.environment_id = r.environment_id
                         AND e.draft_revision_id = r.revision_id
                         AND e.deleted_at IS NULL AND e.id <> entry)
               OR EXISTS (SELECT 1 FROM public.publications p
                          WHERE p.project_id = r.project_id AND p.environment_id = r.environment_id
                            AND p.revision_id = r.revision_id AND p.valid_to IS NULL
                            AND p.entry_id <> entry))
    ) THEN
        RETURN QUERY SELECT 'referenced'::text, draft, NULL::timestamptz;
        RETURN;
    END IF;
    -- A changeset that hasn't been published (scheduled or not) would publish it later.
    IF EXISTS (
        SELECT 1 FROM public.changeset_items i
        WHERE i.project_id = project AND i.environment_id = environment AND i.entry_id = entry
          AND NOT EXISTS (SELECT 1 FROM public.releases rel
                          WHERE rel.project_id = i.project_id
                            AND rel.environment_id = i.environment_id
                            AND rel.changeset_id = i.changeset_id)
    ) THEN
        RETURN QUERY SELECT 'in_changeset'::text, draft, NULL::timestamptz;
        RETURN;
    END IF;
    UPDATE public.entries e SET deleted_at = now(), deleted_by = actor
    WHERE e.project_id = project AND e.environment_id = environment AND e.id = entry;
    DELETE FROM public.unique_claims c
    WHERE c.project_id = project AND c.environment_id = environment AND c.entry_id = entry;
    INSERT INTO public.audit_events (project_id, id, actor_id, action, subject, detail)
    VALUES (project, event, actor, 'entry.deleted', entry::text,
            jsonb_build_object('collection', colophon_delete_entry.collection,
                               'title', title, 'revision_id', draft));
    RETURN QUERY SELECT 'deleted'::text, draft, now();
END
$$;

ALTER FUNCTION colophon_delete_entry(uuid, uuid, uuid, text, uuid, uuid) OWNER TO colophon_deleter;
REVOKE ALL ON FUNCTION colophon_delete_entry(uuid, uuid, uuid, text, uuid, uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_delete_entry(uuid, uuid, uuid, text, uuid, uuid) TO colophon_api;

-- Deleting is idempotent, like saving.
ALTER TABLE idempotency_records DROP CONSTRAINT idempotency_records_operation_check;
ALTER TABLE idempotency_records ADD CONSTRAINT idempotency_records_operation_check
    CHECK (operation IN ('entry.create', 'entry.save', 'entry.delete', 'content_type.create',
                         'content_type.update', 'changeset.create', 'changeset.publish',
                         'component.create', 'component.update'));
