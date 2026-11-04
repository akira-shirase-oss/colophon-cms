-- Milestone P6: the deletion ledger. docs/modules/02-db.md ("The deletion ledger"),
-- docs/modules/18-operations.md ("Backups", "Restore procedure"; invariant O2), F14 in
-- docs/modules/17-verification.md, D-124.
--
-- A restore brings back whatever the backup held. Anything deleted *after* the backup's
-- timestamp — removed outright or tombstoned — comes back alive, because the deletion itself is
-- what the backup is missing. The deletion ledger is the append-only record of those deletions:
-- every path that permanently removes a governed object appends a row in the same transaction,
-- the ledger is copied off-host (its export rides the backup manifest, spec §32), and the
-- restore procedure replays the interval the backup postdates before anything serves (spec §33,
-- restore step 4; O2). F14's "deleted project appears in a restored backup" is the case this
-- exists for.
--
-- What counts as a deletion: the object is gone from the live set — a collection's rows removed
-- (D-083), a membership, a custom role, a project (below), or a governed object retired to its
-- terminal state (an entry's tombstone, D-080; an asset's `deleted`). A tombstone survives a
-- *logical* read of the database, but not a restore to before it was written, so both kinds are
-- recorded. What is deliberately not here: revocations and state flags on rows that stay (a
-- revoked invitation or token, a resolved comment), and the `tombstones` unpublish history —
-- publications a restore reopens are re-closed by the recovery epoch and the external-effects
-- reconciliation (restore steps 3 and 5), not by deleting rows.
--
-- `project_id` is deliberately not a foreign key: the ledger outlives the project it names, and
-- a replay must find the row after the project is gone. `project_slug` rides along so a human
-- reading the file off-host can name what was deleted. Rows are ordered by an identity column,
-- so an export can carry a high-water mark and a restore can verify the interval it needs is
-- complete (O2's "blocked until the gap is resolved"): a row committed later can never land
-- below a watermark already exported.
--
-- The actor a replay attributes its rewrites to (like the worker's identity, migration 0045):
INSERT INTO identities (id, issuer, subject, display_name)
VALUES ('a1b2c3d4-0000-4000-8000-000000000007', 'urn:colophon:system', 'restorer', 'Colophon restore')
ON CONFLICT (id) DO NOTHING;

CREATE TABLE deletion_ledger (
    -- Ordered, monotone: a gap can follow a rolled-back insert and is harmless — an interval is
    -- checked complete by its watermark, never by counting.
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    deleted_at timestamptz NOT NULL DEFAULT now(),
    kind text NOT NULL CHECK (kind IN ('project', 'collection', 'entry', 'member', 'role', 'asset')),
    project_id uuid NOT NULL,
    project_slug text NOT NULL,
    -- Entries, collections and assets name their environment; projects, members and roles don't.
    environment_id uuid,
    -- What was deleted, enough to reapply it: {collection, entry}, {collection}, {identity},
    -- {role}, {asset}, {slug, name}. Never content.
    detail jsonb NOT NULL DEFAULT '{}' CHECK (octet_length(detail::text) <= 8192)
);

-- Append-only, like audit_events: the API may insert rows for its own project context (the
-- SECURITY DEFINER writers run as colophon_deleter, a member), the worker may append only the
-- asset `deleted` outcomes it lands, and nothing may read, update or delete through the
-- application roles. Reads and the off-host export belong to the migration login — which is why
-- row-level security is enabled but not forced: the table owner bypasses it, exactly as
-- api_tokens already does for its pre-context lookup.
ALTER TABLE deletion_ledger ENABLE ROW LEVEL SECURITY;
CREATE POLICY deletion_ledger_writes ON deletion_ledger FOR INSERT TO colophon_api
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
CREATE POLICY deletion_ledger_worker_assets ON deletion_ledger FOR INSERT TO colophon_worker
    WITH CHECK (kind = 'asset');

CREATE FUNCTION colophon_ledger_append_only() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'the deletion ledger is append-only: % refused', tg_op
        USING ERRCODE = 'insufficient_privilege';
END
$$;
CREATE TRIGGER deletion_ledger_append_only BEFORE UPDATE OR DELETE ON deletion_ledger
    FOR EACH ROW EXECUTE FUNCTION colophon_ledger_append_only();
CREATE TRIGGER deletion_ledger_no_truncate BEFORE TRUNCATE ON deletion_ledger
    FOR EACH STATEMENT EXECUTE FUNCTION colophon_ledger_append_only();

GRANT INSERT ON deletion_ledger TO colophon_api;
GRANT INSERT ON deletion_ledger TO colophon_worker;

-- The replay copies each row in with its own id. That asks for ON CONFLICT, which reads the
-- conflict column — SELECT the application roles must never have on this table — so the copy
-- is a SECURITY DEFINER function owned by the migrator instead: the ledger stays invisible to
-- them, and only colophon_deleter may run it, and only for the replay's one purpose.
CREATE FUNCTION colophon_ledger_row(
    id bigint, deleted_at timestamptz, kind text, project uuid, slug text,
    environment uuid, detail jsonb)
    RETURNS void
    LANGUAGE sql
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
    INSERT INTO public.deletion_ledger
        (id, deleted_at, kind, project_id, project_slug, environment_id, detail)
    OVERRIDING SYSTEM VALUE
    VALUES (colophon_ledger_row.id, colophon_ledger_row.deleted_at, colophon_ledger_row.kind,
            colophon_ledger_row.project, colophon_ledger_row.slug,
            colophon_ledger_row.environment, colophon_ledger_row.detail)
    ON CONFLICT (id) DO NOTHING;
$$;
REVOKE ALL ON FUNCTION colophon_ledger_row(bigint, timestamptz, text, uuid, text, uuid, jsonb)
    FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_ledger_row(bigint, timestamptz, text, uuid, text, uuid, jsonb)
    TO colophon_deleter;

-- The worker names the project in its rows (the ledger insert reads the slug, keyed on the id);
-- the column grant keeps it to just those two.
CREATE POLICY projects_for_workers ON projects FOR SELECT TO colophon_worker USING (true);
GRANT SELECT (id, slug) ON projects TO colophon_worker;

-- Deleting an entry (migration 0030) now records it in the same transaction. Only the body's
-- last statements change; the signature is identical, so CREATE OR REPLACE is enough.
CREATE OR REPLACE FUNCTION colophon_delete_entry(
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
    -- The ledger row that would reapply this tombstone on a restored backup (migration 0048).
    INSERT INTO public.deletion_ledger (kind, project_id, project_slug, environment_id, detail)
    VALUES ('entry', colophon_delete_entry.project,
            (SELECT p.slug FROM public.projects p
             WHERE p.id = colophon_delete_entry.project),
            colophon_delete_entry.environment,
            jsonb_build_object('collection', colophon_delete_entry.collection,
                               'entry', entry, 'revision_id', draft));
    RETURN QUERY SELECT 'deleted'::text, draft, now();
END
$$;

ALTER FUNCTION colophon_delete_entry(uuid, uuid, uuid, text, uuid, uuid) OWNER TO colophon_deleter;
REVOKE ALL ON FUNCTION colophon_delete_entry(uuid, uuid, uuid, text, uuid, uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_delete_entry(uuid, uuid, uuid, text, uuid, uuid) TO colophon_api;

-- Deleting a project removes it for good: every row it governed goes, in the order the foreign
-- keys demand, inside the caller's transaction — the same discipline as
-- colophon_delete_entry (context checked inside; the API decides who may call it, a fresh
-- authentication per module 03). The deletion ledger's row is written first, because it is the
-- record that survives: the project's own audit trail goes with it, which is also why
-- `deleted_at` on a project is not the model — a tombstone would keep every byte the owner
-- asked to erase while hiding them from everyone. There is no public endpoint yet; the
-- function is here so the replay path and a later delete route share one implementation.
--
-- Three guards bend for exactly one transaction shape, flagged by `app.deleting_project`
-- carrying the project's id until the transaction ends (the deferred owner check reads it at
-- commit, so the function does not clear it): the audit log's append-only trigger (the trail
-- goes with the project), the fixed owner role's trigger (the role goes with the project) and
-- the last-owner check (its members go with the project). Each stays in force for every other
-- project and for everyone else. `app.ledger_replay` carries a replay's mark for the same
-- last-owner check: replaying a member removal onto a backup that predates the new owner's
-- membership would otherwise be refused by the deferred trigger.
CREATE OR REPLACE FUNCTION colophon_audit_append_only() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    -- Migration 0048: a project being deleted takes its audit trail with it; the deletion
    -- ledger, copied off-host, is the record that survives. Nested, not one AND: OLD is
    -- unassigned for the TRUNCATE trigger, and SQL does not promise short-circuit.
    IF tg_op = 'DELETE' THEN
        IF nullif(current_setting('app.deleting_project', true), '')::uuid = OLD.project_id THEN
            RETURN OLD;
        END IF;
    END IF;
    RAISE EXCEPTION 'the audit log is append-only: % refused', tg_op
        USING ERRCODE = 'insufficient_privilege';
END
$$;

CREATE OR REPLACE FUNCTION colophon_owner_role_is_fixed() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF nullif(current_setting('app.deleting_project', true), '')::uuid = OLD.project_id THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'the owner role is fixed: it always holds every permission'
        USING ERRCODE = 'insufficient_privilege';
END
$$;

CREATE OR REPLACE FUNCTION memberships_keep_an_owner() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    -- A project being deleted loses its members outright, and a ledger replay may remove the
    -- last owner of a project its backup predates (the new owner joined after it) — both are
    -- removals the ledger already recorded, so the check yields only to those transactions.
    IF nullif(current_setting('app.deleting_project', true), '')::uuid IS DISTINCT FROM OLD.project_id
       AND current_setting('app.ledger_replay', true) IS DISTINCT FROM 'on'
       AND NOT EXISTS (SELECT 1 FROM public.memberships
                       WHERE project_id = OLD.project_id AND role = 'owner') THEN
        RAISE EXCEPTION 'a project must keep at least one owner' USING ERRCODE = 'check_violation';
    END IF;
    RETURN NULL;
END
$$;

CREATE FUNCTION colophon_delete_project(project uuid)
    RETURNS TABLE (outcome text)
    LANGUAGE plpgsql
    VOLATILE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    actor uuid := nullif(current_setting('app.identity_id', true), '')::uuid;
    slug text;
    title text;
BEGIN
    IF actor IS NULL
       OR project IS DISTINCT FROM nullif(current_setting('app.project_id', true), '')::uuid THEN
        RAISE EXCEPTION 'colophon_delete_project runs only inside its own project''s context'
            USING ERRCODE = 'insufficient_privilege';
    END IF;
    SELECT p.slug, p.name INTO slug, title
    FROM public.projects p WHERE p.id = colophon_delete_project.project;
    IF NOT FOUND THEN
        RETURN QUERY SELECT 'not_found'::text;
        RETURN;
    END IF;
    -- The record first, then the truth of it: the ledger row survives the project; the audit
    -- trail does not, by choice — a deleted project's history is the ledger's, off-host.
    INSERT INTO public.deletion_ledger (kind, project_id, project_slug, environment_id, detail)
    VALUES ('project', colophon_delete_project.project, slug, NULL,
            jsonb_build_object('slug', slug, 'name', title));
    PERFORM set_config('app.deleting_project', colophon_delete_project.project::text, true);
    -- Children before parents, as the foreign keys require. A table a later migration adds in a
    -- project's scope makes this fail loudly on its key, which is the safe direction.
    DELETE FROM public.observations o WHERE o.project_id = colophon_delete_project.project;
    DELETE FROM public.budget_reservations b WHERE b.project_id = colophon_delete_project.project;
    DELETE FROM public.jobs j WHERE j.project_id = colophon_delete_project.project;
    DELETE FROM public.asset_references a WHERE a.project_id = colophon_delete_project.project;
    DELETE FROM public.asset_jobs a WHERE a.project_id = colophon_delete_project.project;
    DELETE FROM public.tombstones t WHERE t.project_id = colophon_delete_project.project;
    DELETE FROM public.publications p WHERE p.project_id = colophon_delete_project.project;
    DELETE FROM public.revision_references r WHERE r.project_id = colophon_delete_project.project;
    DELETE FROM public.unique_claims u WHERE u.project_id = colophon_delete_project.project;
    DELETE FROM public.comments c WHERE c.project_id = colophon_delete_project.project;
    DELETE FROM public.tasks t WHERE t.project_id = colophon_delete_project.project;
    DELETE FROM public.migration_items m WHERE m.project_id = colophon_delete_project.project;
    DELETE FROM public.bulk_items b WHERE b.project_id = colophon_delete_project.project;
    DELETE FROM public.bulk_jobs b WHERE b.project_id = colophon_delete_project.project;
    DELETE FROM public.schedules s WHERE s.project_id = colophon_delete_project.project;
    DELETE FROM public.releases r WHERE r.project_id = colophon_delete_project.project;
    DELETE FROM public.reviews r WHERE r.project_id = colophon_delete_project.project;
    DELETE FROM public.changeset_items c WHERE c.project_id = colophon_delete_project.project;
    DELETE FROM public.changesets c WHERE c.project_id = colophon_delete_project.project;
    DELETE FROM public.review_stages r WHERE r.project_id = colophon_delete_project.project;
    DELETE FROM public.destinations d WHERE d.project_id = colophon_delete_project.project;
    DELETE FROM public.assets a WHERE a.project_id = colophon_delete_project.project;
    DELETE FROM public.revisions r WHERE r.project_id = colophon_delete_project.project;
    DELETE FROM public.entries e WHERE e.project_id = colophon_delete_project.project;
    DELETE FROM public.migration_plans m WHERE m.project_id = colophon_delete_project.project;
    DELETE FROM public.content_type_versions c WHERE c.project_id = colophon_delete_project.project;
    DELETE FROM public.collections c WHERE c.project_id = colophon_delete_project.project;
    DELETE FROM public.component_versions c WHERE c.project_id = colophon_delete_project.project;
    DELETE FROM public.components c WHERE c.project_id = colophon_delete_project.project;
    DELETE FROM public.idempotency_records i WHERE i.project_id = colophon_delete_project.project;
    DELETE FROM public.environments e WHERE e.project_id = colophon_delete_project.project;
    DELETE FROM public.api_token_limits a WHERE a.project_id = colophon_delete_project.project;
    DELETE FROM public.api_token_permissions a WHERE a.project_id = colophon_delete_project.project;
    DELETE FROM public.api_tokens a WHERE a.project_id = colophon_delete_project.project;
    DELETE FROM public.audit_events a WHERE a.project_id = colophon_delete_project.project;
    DELETE FROM public.memberships m WHERE m.project_id = colophon_delete_project.project;
    DELETE FROM public.invitations i WHERE i.project_id = colophon_delete_project.project;
    DELETE FROM public.roles r WHERE r.project_id = colophon_delete_project.project;
    DELETE FROM public.projects p WHERE p.id = colophon_delete_project.project;
    -- `app.deleting_project` deliberately stays set: the deferred last-owner trigger reads it
    -- at commit. The setting is transaction-local, so it cannot leak to another transaction,
    -- and it names this project only — every guard keeps its force for everything else.
    RETURN QUERY SELECT 'deleted'::text;
END
$$;

ALTER FUNCTION colophon_delete_project(uuid) OWNER TO colophon_deleter;
REVOKE ALL ON FUNCTION colophon_delete_project(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_delete_project(uuid) TO colophon_api;

-- What the function needs beyond colophon_api's grants (which it inherits by membership): the
-- split entries policies get a delete of their own, observations and idempotency records have
-- no project-wide delete, and projects has no delete at all.
CREATE POLICY entries_deleter_removes ON entries FOR DELETE TO colophon_deleter
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
CREATE POLICY observations_deleter ON observations FOR DELETE TO colophon_deleter
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
CREATE POLICY idempotency_records_deleter ON idempotency_records FOR DELETE TO colophon_deleter
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
CREATE POLICY projects_deleter ON projects FOR DELETE TO colophon_deleter
    USING (id = nullif(current_setting('app.project_id', true), '')::uuid);

GRANT DELETE ON observations, budget_reservations, jobs, asset_references, asset_jobs,
    tombstones, publications, revision_references, unique_claims, comments, tasks,
    migration_items, migration_plans, bulk_items, bulk_jobs, schedules, releases, reviews,
    changeset_items, changesets, review_stages, destinations, assets, revisions, entries,
    content_type_versions, collections, component_versions, components, idempotency_records,
    environments, api_token_limits, api_token_permissions, api_tokens, audit_events,
    memberships, invitations, roles, projects
    TO colophon_deleter;
-- The replay's asset tombstone writes only `state`.
GRANT UPDATE (state) ON assets TO colophon_deleter;

-- The migrator replays the ledger as colophon_deleter (SET LOCAL ROLE): grant the membership.
-- In production the owner role created colophon_deleter, so it holds its admin option; a
-- refusal is warned, not fatal — the grant can be repeated by hand for the restore procedure.
DO $$
BEGIN
    EXECUTE format('GRANT colophon_deleter TO %I WITH INHERIT FALSE, SET TRUE', session_user);
EXCEPTION WHEN insufficient_privilege OR invalid_grant_operation OR duplicate_object THEN
    RAISE WARNING 'could not grant colophon_deleter to %: ledger replay needs it', session_user;
END
$$;
