-- Milestone P6, media retention step 1: staged deletion of assets. docs/modules/07-media.md,
-- D-098.
--
-- An asset row is never removed. A DELETE request checks the state, counts who still names the
-- asset — a live entry's draft, or any publication of it at all (publications pin forever,
-- D-098) — and commits only an intent: an asset_jobs row of kind 'delete' beside an audit
-- event, so 202 means requested, not gone. The worker claims the job, locks the asset FOR
-- UPDATE, rechecks the same rule under the lock — the authority, because its lock is what new
-- references and publications wait on — marks the asset 'deleting', and only then, outside any
-- transaction, removes every object it ever wrote: the public key, each rendition under it,
-- and the quarantine object it started as. A crash anywhere in that list retries the job;
-- object removals are idempotent and 'deleting' says which remain.
--
-- Two triggers close the race between "a delete is on its way" and "someone just named it".
-- Both are SECURITY DEFINER and owned by colophon_worker: a row lock needs an UPDATE grant on
-- the table — PostgreSQL requires one for every lock mode, FOR KEY SHARE included (verified on
-- PostgreSQL 18) — and the API role deliberately has none on assets (0035, D-094); the worker
-- is the only role that may both hold and change them. The check inside runs under the
-- worker's FOR ALL policies, so it sees the job row whoever's transaction it is in.
--
-- `colophon_asset_live` guards new references: a save's asset_references row locks the named
-- asset FOR KEY SHARE — waiting out a worker's FOR UPDATE — then refuses when the asset is
-- already 'deleting' or 'deleted', or a delete/expire job of any non-terminal status exists
-- (the delete was requested; the asset just doesn't know yet). The error is a foreign-key
-- violation naming `<table>_asset_live` with the asset's id as its detail, so a save answers
-- `asset_not_found` exactly as for an asset that never was.
CREATE FUNCTION colophon_asset_live() RETURNS trigger
    LANGUAGE plpgsql
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    found_state text;
BEGIN
    SELECT a.state INTO found_state FROM public.assets a
    WHERE a.project_id = NEW.project_id AND a.environment_id = NEW.environment_id
      AND a.id = NEW.asset_id
    FOR KEY SHARE;
    IF NOT FOUND OR found_state IN ('deleting', 'deleted') THEN
        RAISE EXCEPTION 'asset % is deleted or does not exist', NEW.asset_id
            USING ERRCODE = 'foreign_key_violation',
                  CONSTRAINT = tg_table_name || '_asset_live',
                  TABLE = tg_table_name,
                  DETAIL = NEW.asset_id::text;
    END IF;
    IF EXISTS (
        SELECT 1 FROM public.asset_jobs j
        WHERE j.project_id = NEW.project_id AND j.environment_id = NEW.environment_id
          AND j.asset_id = NEW.asset_id AND j.kind IN ('delete', 'expire')
          AND j.status IN ('pending', 'running', 'retrying')
    ) THEN
        RAISE EXCEPTION 'asset % is being deleted', NEW.asset_id
            USING ERRCODE = 'foreign_key_violation',
                  CONSTRAINT = tg_table_name || '_asset_live',
                  TABLE = tg_table_name,
                  DETAIL = NEW.asset_id::text;
    END IF;
    RETURN NEW;
END
$$;
ALTER FUNCTION colophon_asset_live() OWNER TO colophon_worker;
REVOKE ALL ON FUNCTION colophon_asset_live() FROM PUBLIC;

CREATE TRIGGER asset_references_asset_live BEFORE INSERT ON asset_references
    FOR EACH ROW EXECUTE FUNCTION colophon_asset_live();

-- `colophon_publication_assets` only locks: a release's publications row takes FOR KEY SHARE
-- on every asset its revision names, so a publish either wins the lock before a delete takes
-- the asset's FOR UPDATE — the worker's recheck then sees the publication and refuses — or
-- waits out the 'deleting' commit and is refused by the same unusable-asset check as any
-- non-usable upload. The lock itself is the whole guarantee; checking the state here would
-- only duplicate bad_asset, which reads it after the wait.
CREATE FUNCTION colophon_publication_assets() RETURNS trigger
    LANGUAGE plpgsql
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    named record;
BEGIN
    FOR named IN
        SELECT r.asset_id FROM public.asset_references r
        WHERE r.project_id = NEW.project_id AND r.environment_id = NEW.environment_id
          AND r.revision_id = NEW.revision_id
        ORDER BY r.asset_id
    LOOP
        PERFORM 1 FROM public.assets a
        WHERE a.project_id = NEW.project_id AND a.environment_id = NEW.environment_id
          AND a.id = named.asset_id
        FOR KEY SHARE;
    END LOOP;
    RETURN NEW;
END
$$;
ALTER FUNCTION colophon_publication_assets() OWNER TO colophon_worker;
REVOKE ALL ON FUNCTION colophon_publication_assets() FROM PUBLIC;

CREATE TRIGGER publications_asset_live BEFORE INSERT ON publications
    FOR EACH ROW EXECUTE FUNCTION colophon_publication_assets();

-- The tombstone states, and the kinds that move an asset through them: 'delete' now, 'expire'
-- for the orphaned-upload sweep retention step 2 adds.
ALTER TABLE assets DROP CONSTRAINT assets_state_check;
ALTER TABLE assets ADD CONSTRAINT assets_state_check
    CHECK (state IN ('quarantined', 'usable', 'rejected', 'deleting', 'deleted'));
ALTER TABLE asset_jobs DROP CONSTRAINT asset_jobs_kind_check;
ALTER TABLE asset_jobs ADD CONSTRAINT asset_jobs_kind_check
    CHECK (kind IN ('verify', 'delete', 'expire'));

-- What the sweep walks: collectable states by age.
CREATE INDEX assets_collectable ON assets (state, created_at);

-- The worker's recheck reads who names the asset: references, the entries whose drafts use
-- them (live ones only — it filters deleted_at itself), and publications, which it already
-- reads for delivery jobs (0009). It sees every project's rows, as it does on asset_jobs.
CREATE POLICY asset_references_for_workers ON asset_references FOR SELECT TO colophon_worker
    USING (true);
GRANT SELECT ON asset_references TO colophon_worker;
CREATE POLICY entries_for_workers ON entries FOR SELECT TO colophon_worker USING (true);
GRANT SELECT (project_id, environment_id, id, draft_revision_id, deleted_at) ON entries
    TO colophon_worker;
-- The retention sweeps write their own jobs (step 2); delete requests come from the API.
GRANT INSERT ON asset_jobs TO colophon_worker;

-- The deletion request claims its key like every other write.
ALTER TABLE idempotency_records DROP CONSTRAINT idempotency_records_operation_check;
ALTER TABLE idempotency_records ADD CONSTRAINT idempotency_records_operation_check
    CHECK (operation IN ('entry.create', 'entry.save', 'entry.delete', 'entry.import',
                         'entry.localize',
                         'content_type.create', 'content_type.update', 'content_type.delete',
                         'changeset.create', 'changeset.publish',
                         'component.create', 'component.update',
                         'asset.upload', 'asset.delete'));
