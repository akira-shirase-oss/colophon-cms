-- D-177: change hints leave the writer's commit.
--
-- Migration 0044's row triggers called `pg_notify` inside every watched write. PostgreSQL
-- serialises every committing transaction that issued a NOTIFY on one cluster-wide lock, held
-- from just before the commit until its WAL is flushed — so under write load each such commit
-- waited for the previous one's fsync, one at a time, and group commit was lost. CI's load
-- smokes showed it: members, schedules and tokens commits queued 100 to 380 ms on that lock
-- (`AccessExclusiveLock on object 0 of class 1262`).
--
-- Now a write only appends its hints to `change_hints`: an UNLOGGED table (no WAL, so no extra
-- flush), no index, filled by statement-level triggers that add one row per distinct hint per
-- statement — a 1,000-row write adds one row, not 1,000. A pump in every Colophon process
-- (`colophon_db::events::pump`) drains the committed rows every 100 ms and sends each distinct
-- payload once, in one short transaction of its own: the NOTIFY lock is taken by the pump a few
-- times a second, never by an editor's save. A hint still exists only for a commit (the pump
-- sees committed rows only), the payload format is unchanged, and listeners are unchanged.
-- Being unlogged, pending hints vanish in a crash; a listener's reconnect already resyncs every
-- stream, so a lost hint only ever meant a reload, never stale data.

CREATE UNLOGGED TABLE change_hints (
    payload text NOT NULL CHECK (octet_length(payload) <= 400)
);
-- Every role that writes a watched table writes its hints; only the pump (as the application)
-- reads and deletes them (FOR UPDATE SKIP LOCKED needs UPDATE).
GRANT INSERT ON change_hints TO colophon_api, colophon_worker, colophon_deleter, colophon_retention;
GRANT SELECT, UPDATE, DELETE ON change_hints TO colophon_api, colophon_worker;

-- The hint for each distinct changed row of the statement, in 0044's format:
-- `p=<project>|e=<environment|-->|k=<kind>[|c=<collection>]`. The transition table is named
-- `changed` by every trigger below.
CREATE FUNCTION colophon_hint() RETURNS trigger
    LANGUAGE plpgsql
    SET search_path = pg_catalog, public
AS $$
BEGIN
    INSERT INTO public.change_hints (payload)
    SELECT DISTINCT
        'p=' || (doc ->> 'project_id')
            || '|e=' || coalesce(doc ->> 'environment_id', '--')
            || '|k=' || TG_ARGV[0]
            || coalesce('|c=' || nullif(doc ->> TG_ARGV[1], ''), '')
    FROM (SELECT to_jsonb(c) AS doc FROM changed c) AS docs;
    RETURN NULL;
END;
$$;

COMMENT ON FUNCTION colophon_hint() IS
    'D-177: queue one change hint per distinct changed row of a statement; the pump notifies.';

-- 0044's watched tables, their kinds, the collection column ('' when none), and whether they
-- see updates and deletes or only inserts.
DO $$
DECLARE
    watched record;
BEGIN
    FOR watched IN
        SELECT * FROM (VALUES
            ('entries', 'entries', 'collection', true),
            ('publications', 'entries', 'collection', true),
            ('tombstones', 'entries', 'collection', false),
            ('collections', 'types', '', true),
            ('content_type_versions', 'types', '', true),
            ('components', 'components', '', true),
            ('component_versions', 'components', '', true),
            ('changesets', 'changesets', '', true),
            ('changeset_items', 'changesets', '', true),
            ('reviews', 'changesets', '', true),
            ('review_stages', 'stages', '', true),
            ('releases', 'releases', '', true),
            ('jobs', 'releases', '', true),
            ('observations', 'releases', '', false),
            ('bulk_jobs', 'releases', '', true),
            ('bulk_items', 'releases', '', true),
            ('schedules', 'schedules', '', true),
            ('tasks', 'tasks', 'collection', true),
            ('comments', 'comments', 'collection', true),
            ('migration_plans', 'migrations', '', true),
            ('migration_items', 'migrations', '', true),
            ('assets', 'assets', '', true),
            ('asset_jobs', 'assets', '', true),
            ('asset_references', 'assets', '', true),
            ('destinations', 'destinations', '', true),
            ('memberships', 'members', '', true),
            ('invitations', 'members', '', true),
            ('roles', 'roles', '', true),
            ('role_permissions', 'roles', '', true),
            ('api_tokens', 'tokens', '', true),
            ('api_token_permissions', 'tokens', '', true),
            ('api_token_limits', 'tokens', '', true),
            ('audit_events', 'audit', '', false)
        ) AS t (tbl, kind, col, changes)
    LOOP
        EXECUTE format('DROP TRIGGER %I ON %I', watched.tbl || '_notify', watched.tbl);
        EXECUTE format(
            'CREATE TRIGGER %I AFTER INSERT ON %I REFERENCING NEW TABLE AS changed
                 FOR EACH STATEMENT EXECUTE FUNCTION colophon_hint(%L, %L)',
            watched.tbl || '_hint_insert', watched.tbl, watched.kind, watched.col);
        IF watched.changes THEN
            EXECUTE format(
                'CREATE TRIGGER %I AFTER UPDATE ON %I REFERENCING NEW TABLE AS changed
                     FOR EACH STATEMENT EXECUTE FUNCTION colophon_hint(%L, %L)',
                watched.tbl || '_hint_update', watched.tbl, watched.kind, watched.col);
            EXECUTE format(
                'CREATE TRIGGER %I AFTER DELETE ON %I REFERENCING OLD TABLE AS changed
                     FOR EACH STATEMENT EXECUTE FUNCTION colophon_hint(%L, %L)',
                watched.tbl || '_hint_delete', watched.tbl, watched.kind, watched.col);
        END IF;
    END LOOP;
END
$$;

-- Nothing calls the row-level notifier any more.
DROP FUNCTION colophon_notify();
