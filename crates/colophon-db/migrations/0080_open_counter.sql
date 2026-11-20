-- D-216 phase 2: a project's open work is a running count, checked at the end of an admission.
--
-- Until now every job-admitting transaction took the project's advisory lock (class 0x5741, "WA")
-- before its writes and held it to commit, and counted the project's open work across four tables
-- under it: a project's admissions ran one at a time across every round trip of the request.
--
-- Now, as 0078 did for the installation's payload:
--
-- 1. Statement-level triggers on the four tables the bound counts (jobs, asset_jobs, bulk_jobs,
--    migration_plans) append each statement's change in open rows to `open_deltas`, per project.
--    Workers that claim, finish, sweep or supersede work only ever insert here.
-- 2. `work_open` holds one row per project, made with the project: the count folded so far. The
--    count is that row plus the project's deltas not yet folded.
-- 3. An admission checks at the END of its transaction, after its own writes:
--    `colophon_work_admit()` locks its project's row (held to commit), folds every delta of the
--    project it can see into it — its own uncommitted ones included — and returns the count. The
--    next admission of the project waits on that row lock and then, in a fresh statement, sees
--    this one's committed fold, so two admissions can never both see room that isn't there. A
--    rollback undoes the fold with everything else.
-- 4. The worker's tick folds too (`colophon_work_tick`), skipping a project whose row an
--    admission holds.
--
-- What counts as open is unchanged (`work::open`, `assets::enqueue_expired`): delivery jobs
-- pending, retrying, running or held; asset jobs pending, running or retrying; bulk jobs
-- accepting or running; plans proposed, approved or running. `colophon_work_reconcile()` recounts
-- them and reports any drift; nothing repairs it.
--
-- Lock order: the project's row is taken last but one, by the admission's final check, and the
-- installation's payload counter (0078) last of all. Nobody waits on anything while holding
-- either, so no wait can circle.

CREATE TABLE work_open (
    project_id uuid PRIMARY KEY,
    open bigint NOT NULL
);

-- Append-only between folds; a fold deletes the project's deltas it can see. No foreign key: a
-- cascade from a deleted project fires these triggers after the project row is gone.
CREATE TABLE open_deltas (
    project_id uuid NOT NULL,
    delta bigint NOT NULL,
    at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX open_deltas_by_project ON open_deltas (project_id);

-- The true count, project by project: the backfill's and the reconciliation's truth.
CREATE FUNCTION colophon_work_actual(p uuid)
    RETURNS bigint
    LANGUAGE sql
    STABLE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
    SELECT count(*)::bigint FROM (
        SELECT 1 FROM public.jobs
        WHERE project_id = p AND status IN ('pending', 'retrying', 'running', 'held')
        UNION ALL
        SELECT 1 FROM public.asset_jobs
        WHERE project_id = p AND status IN ('pending', 'running', 'retrying')
        UNION ALL
        SELECT 1 FROM public.bulk_jobs
        WHERE project_id = p AND state IN ('accepting', 'running')
        UNION ALL
        SELECT 1 FROM public.migration_plans
        WHERE project_id = p AND state IN ('proposed', 'approved', 'running')
    ) retained
$$;

-- One function per table, each reading only the transition tables its event has, writing one
-- delta per project the statement changed. They run as colophon_worker, whichever role wrote.

CREATE FUNCTION colophon_open_jobs() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        INSERT INTO public.open_deltas (project_id, delta)
        SELECT project_id, count(*) FROM added
        WHERE status IN ('pending', 'retrying', 'running', 'held') GROUP BY project_id;
    ELSIF TG_OP = 'DELETE' THEN
        INSERT INTO public.open_deltas (project_id, delta)
        SELECT project_id, -count(*) FROM removed
        WHERE status IN ('pending', 'retrying', 'running', 'held') GROUP BY project_id;
    ELSE
        INSERT INTO public.open_deltas (project_id, delta)
        SELECT project_id, sum(d) FROM (
            SELECT project_id, 1 AS d FROM added
            WHERE status IN ('pending', 'retrying', 'running', 'held')
            UNION ALL
            SELECT project_id, -1 FROM removed
            WHERE status IN ('pending', 'retrying', 'running', 'held')
        ) moved GROUP BY project_id HAVING sum(d) <> 0;
    END IF;
    RETURN NULL;
END
$$;

CREATE FUNCTION colophon_open_asset_jobs() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        INSERT INTO public.open_deltas (project_id, delta)
        SELECT project_id, count(*) FROM added
        WHERE status IN ('pending', 'running', 'retrying') GROUP BY project_id;
    ELSIF TG_OP = 'DELETE' THEN
        INSERT INTO public.open_deltas (project_id, delta)
        SELECT project_id, -count(*) FROM removed
        WHERE status IN ('pending', 'running', 'retrying') GROUP BY project_id;
    ELSE
        INSERT INTO public.open_deltas (project_id, delta)
        SELECT project_id, sum(d) FROM (
            SELECT project_id, 1 AS d FROM added WHERE status IN ('pending', 'running', 'retrying')
            UNION ALL
            SELECT project_id, -1 FROM removed WHERE status IN ('pending', 'running', 'retrying')
        ) moved GROUP BY project_id HAVING sum(d) <> 0;
    END IF;
    RETURN NULL;
END
$$;

CREATE FUNCTION colophon_open_bulk_jobs() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        INSERT INTO public.open_deltas (project_id, delta)
        SELECT project_id, count(*) FROM added
        WHERE state IN ('accepting', 'running') GROUP BY project_id;
    ELSIF TG_OP = 'DELETE' THEN
        INSERT INTO public.open_deltas (project_id, delta)
        SELECT project_id, -count(*) FROM removed
        WHERE state IN ('accepting', 'running') GROUP BY project_id;
    ELSE
        INSERT INTO public.open_deltas (project_id, delta)
        SELECT project_id, sum(d) FROM (
            SELECT project_id, 1 AS d FROM added WHERE state IN ('accepting', 'running')
            UNION ALL
            SELECT project_id, -1 FROM removed WHERE state IN ('accepting', 'running')
        ) moved GROUP BY project_id HAVING sum(d) <> 0;
    END IF;
    RETURN NULL;
END
$$;

CREATE FUNCTION colophon_open_plans() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        INSERT INTO public.open_deltas (project_id, delta)
        SELECT project_id, count(*) FROM added
        WHERE state IN ('proposed', 'approved', 'running') GROUP BY project_id;
    ELSIF TG_OP = 'DELETE' THEN
        INSERT INTO public.open_deltas (project_id, delta)
        SELECT project_id, -count(*) FROM removed
        WHERE state IN ('proposed', 'approved', 'running') GROUP BY project_id;
    ELSE
        INSERT INTO public.open_deltas (project_id, delta)
        SELECT project_id, sum(d) FROM (
            SELECT project_id, 1 AS d FROM added WHERE state IN ('proposed', 'approved', 'running')
            UNION ALL
            SELECT project_id, -1 FROM removed WHERE state IN ('proposed', 'approved', 'running')
        ) moved GROUP BY project_id HAVING sum(d) <> 0;
    END IF;
    RETURN NULL;
END
$$;

-- Writes wait while the triggers go in and the counts are taken once, so no change can fall
-- between the count and the first trigger (reads go on).
LOCK TABLE jobs, asset_jobs, bulk_jobs, migration_plans IN SHARE ROW EXCLUSIVE MODE;

DO $$
DECLARE
    source record;
BEGIN
    FOR source IN
        SELECT * FROM (VALUES
            ('jobs', 'colophon_open_jobs'),
            ('asset_jobs', 'colophon_open_asset_jobs'),
            ('bulk_jobs', 'colophon_open_bulk_jobs'),
            ('migration_plans', 'colophon_open_plans')
        ) AS t (tbl, fn)
    LOOP
        EXECUTE format(
            'CREATE TRIGGER %I AFTER INSERT ON %I REFERENCING NEW TABLE AS added
                 FOR EACH STATEMENT EXECUTE FUNCTION %I()',
            source.tbl || '_open_insert', source.tbl, source.fn);
        EXECUTE format(
            'CREATE TRIGGER %I AFTER UPDATE ON %I REFERENCING OLD TABLE AS removed NEW TABLE AS added
                 FOR EACH STATEMENT EXECUTE FUNCTION %I()',
            source.tbl || '_open_update', source.tbl, source.fn);
        EXECUTE format(
            'CREATE TRIGGER %I AFTER DELETE ON %I REFERENCING OLD TABLE AS removed
                 FOR EACH STATEMENT EXECUTE FUNCTION %I()',
            source.tbl || '_open_delete', source.tbl, source.fn);
    END LOOP;
END
$$;

-- Every project has its row from its creation on, so neither an admission nor the worker's
-- tick ever inserts one (an insert racing another's uncommitted insert of the same key waits on
-- it, and the tick must never wait on an admission).
CREATE FUNCTION colophon_work_project() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    INSERT INTO public.work_open (project_id, open) VALUES (NEW.id, 0) ON CONFLICT DO NOTHING;
    RETURN NULL;
END
$$;
LOCK TABLE projects IN SHARE ROW EXCLUSIVE MODE;
CREATE TRIGGER projects_work_open AFTER INSERT ON projects
    FOR EACH ROW EXECUTE FUNCTION colophon_work_project();

INSERT INTO work_open (project_id, open)
SELECT p.id, coalesce(c.open, 0) FROM projects p
LEFT JOIN (
    SELECT project_id, count(*) AS open FROM (
        SELECT project_id FROM jobs WHERE status IN ('pending', 'retrying', 'running', 'held')
        UNION ALL
        SELECT project_id FROM asset_jobs WHERE status IN ('pending', 'running', 'retrying')
        UNION ALL
        SELECT project_id FROM bulk_jobs WHERE state IN ('accepting', 'running')
        UNION ALL
        SELECT project_id FROM migration_plans WHERE state IN ('proposed', 'approved', 'running')
    ) retained GROUP BY project_id
) c ON c.project_id = p.id;

-- An admission's check, at the end of its transaction: the project's row locked to commit, every
-- visible delta of the project folded into it (this transaction's own included), the count
-- returned. The project is the scoped transaction's own (`app.project_id`, the setting
-- row-level security reads), never an argument.
CREATE FUNCTION colophon_work_admit()
    RETURNS bigint
    LANGUAGE plpgsql
    VOLATILE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    p uuid := current_setting('app.project_id')::uuid;
    total bigint;
BEGIN
    PERFORM 1 FROM public.work_open WHERE project_id = p FOR UPDATE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'project % has no work_open row', p;
    END IF;
    -- A statement of its own: under READ COMMITTED it sees what the row's previous holder
    -- committed, its fold included, so nothing is counted twice or missed.
    WITH moved AS (DELETE FROM public.open_deltas WHERE project_id = p RETURNING delta)
    UPDATE public.work_open
       SET open = open + (SELECT coalesce(sum(delta), 0) FROM moved)::bigint
     WHERE project_id = p
    RETURNING open INTO total;
    RETURN total;
END
$$;

-- An admission's whole final check in one round trip, in the one lock order: the project's row
-- (`colophon_work_admit`), then the installation's payload counter (`colophon_payload_admit`,
-- 0078), both held to commit.
CREATE FUNCTION colophon_work_check(OUT open bigint, OUT pending bigint)
    LANGUAGE plpgsql
    VOLATILE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    open := public.colophon_work_admit();
    pending := public.colophon_payload_admit();
END
$$;

-- The project's count without a lock: the folded row plus the deltas this transaction can see.
-- An admission's early refusal reads it; it may lag other transactions' uncommitted work, which
-- the admission's own check at commit does not.
CREATE FUNCTION colophon_work_open()
    RETURNS bigint
    LANGUAGE sql
    STABLE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
    SELECT (coalesce((SELECT open FROM public.work_open
                      WHERE project_id = current_setting('app.project_id')::uuid), 0)
          + (SELECT coalesce(sum(delta), 0) FROM public.open_deltas
             WHERE project_id = current_setting('app.project_id')::uuid))::bigint
$$;

-- The worker's fold: every project's visible deltas folded into its row, skipping a project
-- whose row an admission holds (it folds them itself). Returns how many deltas it folded.
CREATE FUNCTION colophon_work_tick()
    RETURNS bigint
    LANGUAGE plpgsql
    VOLATILE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    p uuid;
    n bigint;
    folded bigint := 0;
BEGIN
    FOR p IN SELECT w.project_id FROM public.work_open w
             WHERE EXISTS (SELECT 1 FROM public.open_deltas d WHERE d.project_id = w.project_id)
             FOR UPDATE OF w SKIP LOCKED
    LOOP
        WITH moved AS (DELETE FROM public.open_deltas WHERE project_id = p RETURNING delta),
             applied AS (
                 UPDATE public.work_open
                    SET open = open + (SELECT coalesce(sum(delta), 0) FROM moved)::bigint
                  WHERE project_id = p)
        SELECT count(*) INTO n FROM moved;
        folded := folded + n;
    END LOOP;
    RETURN folded;
END
$$;

-- Every project's running count against the rows it stands for: drift is reported, never
-- repaired. `projects` is how many projects drift, `drift` their summed absolute drift.
CREATE FUNCTION colophon_work_reconcile(OUT projects bigint, OUT drift bigint)
    LANGUAGE sql
    STABLE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
    SELECT count(*) FILTER (WHERE counted <> actual), coalesce(sum(abs(counted - actual)), 0)::bigint
    FROM (
        SELECT k.project_id,
               coalesce((SELECT open FROM public.work_open w WHERE w.project_id = k.project_id), 0)
             + coalesce((SELECT sum(delta) FROM public.open_deltas d
                         WHERE d.project_id = k.project_id), 0) AS counted,
               public.colophon_work_actual(k.project_id) AS actual
        FROM (SELECT project_id FROM public.work_open
              UNION SELECT project_id FROM public.open_deltas
              UNION SELECT project_id FROM public.jobs
              UNION SELECT project_id FROM public.asset_jobs
              UNION SELECT project_id FROM public.bulk_jobs
              UNION SELECT project_id FROM public.migration_plans) k
    ) c
$$;

-- How long the log is, and how old its oldest delta, for the metrics surface.
CREATE FUNCTION colophon_work_log(OUT deltas bigint, OUT oldest_seconds double precision)
    LANGUAGE sql
    STABLE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
    SELECT count(*), coalesce(extract(epoch FROM now() - min(at)), 0)::double precision
    FROM public.open_deltas
$$;

ALTER TABLE work_open OWNER TO colophon_worker;
ALTER TABLE open_deltas OWNER TO colophon_worker;
DO $$
DECLARE
    fn text;
BEGIN
    FOREACH fn IN ARRAY ARRAY[
        'colophon_work_actual(uuid)', 'colophon_work_project()', 'colophon_open_jobs()', 'colophon_open_asset_jobs()',
        'colophon_open_bulk_jobs()', 'colophon_open_plans()', 'colophon_work_admit()',
        'colophon_work_check()', 'colophon_work_open()', 'colophon_work_tick()', 'colophon_work_reconcile()',
        'colophon_work_log()']
    LOOP
        EXECUTE format('ALTER FUNCTION %s OWNER TO colophon_worker', fn);
        EXECUTE format('REVOKE ALL ON FUNCTION %s FROM PUBLIC', fn);
    END LOOP;
END
$$;
GRANT EXECUTE ON FUNCTION colophon_work_admit(), colophon_work_check(), colophon_work_open()
    TO colophon_api, colophon_worker;
GRANT EXECUTE ON FUNCTION colophon_work_tick(), colophon_work_reconcile(), colophon_work_log()
    TO colophon_worker;
