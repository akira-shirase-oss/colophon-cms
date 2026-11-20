-- D-216: the installation's pending payload is a running total, not a SUM per admission.
--
-- Until now every payload-admitting transaction took the installation-wide byte lock (advisory
-- class 0x5742, "WB") at its start and held it to commit, and ran `colophon_pending_payload`'s
-- six-way SUM up to three times inside it: one project's bulk submit queued every other
-- project's, behind scans that grow with the backlog (D-209).
--
-- Now:
--
-- 1. Statement-level triggers on every table the total weighs append the statement's change to
--    `payload_deltas` (one row per statement, never a lock anyone else waits on). Workers that
--    finish, sweep or supersede work only ever insert here.
-- 2. `payload_counter` is one row: the total folded so far. The total is that row plus the
--    deltas not yet folded.
-- 3. An admission checks at the END of its transaction, after its own writes:
--    `colophon_payload_admit()` locks the counter row (held to commit), folds every delta it can
--    see into it — its own uncommitted ones included — and returns the total. The next
--    admission waits on that row lock and then sees this one's committed fold, so two
--    admissions can never both see room that isn't there. A rollback undoes the fold with
--    everything else.
-- 4. The worker's tick folds too (`colophon_payload_tick`), skipping the turn if an admission
--    holds the row, so the log stays short when nothing is admitted.
--
-- What the total weighs is unchanged (0050, D-135): open delivery jobs' keys; open asset jobs'
-- keys plus their asset's recorded size; open bulk jobs' operation documents and their pending
-- items' inputs; open plans' definitions and operations and their items' stored resolutions
-- and problems. A parent's change of state carries its children's bytes in or out with it.
-- `colophon_payload_reconcile()` recomputes that SUM and reports any drift; nothing repairs it.

CREATE TABLE payload_counter (
    id smallint PRIMARY KEY CHECK (id = 1),
    bytes bigint NOT NULL
);

-- Unindexed and append-only between folds: a fold deletes everything it can see.
CREATE TABLE payload_deltas (
    bytes bigint NOT NULL,
    at timestamptz NOT NULL DEFAULT now()
);

-- Open asset jobs by their asset: a recorded size change or an asset's deletion finds them.
CREATE INDEX asset_jobs_open_by_asset ON asset_jobs (project_id, environment_id, asset_id)
    WHERE status IN ('pending', 'running', 'retrying');

-- The SUM 0050 counts, without its lock: the reconciliation's truth.
CREATE FUNCTION colophon_payload_actual()
    RETURNS bigint
    LANGUAGE sql
    STABLE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
    SELECT coalesce(sum(bytes), 0)::bigint FROM (
        SELECT octet_length(idempotency_key) AS bytes FROM public.jobs
        WHERE status IN ('pending', 'retrying', 'running', 'held')
        UNION ALL
        SELECT octet_length(j.idempotency_key) + a.size_bytes FROM public.asset_jobs j
        JOIN public.assets a
          ON a.project_id = j.project_id AND a.environment_id = j.environment_id
         AND a.id = j.asset_id
        WHERE j.status IN ('pending', 'running', 'retrying')
        UNION ALL
        SELECT octet_length(b.operation::text) FROM public.bulk_jobs b
        WHERE b.state IN ('accepting', 'running')
        UNION ALL
        SELECT octet_length(i.input::text) FROM public.bulk_items i
        JOIN public.bulk_jobs b
          ON b.project_id = i.project_id AND b.environment_id = i.environment_id
         AND b.id = i.job_id
        WHERE b.state IN ('accepting', 'running') AND i.outcome = 'pending'
        UNION ALL
        SELECT octet_length(p.target_definition::text) + octet_length(p.operations::text)
        FROM public.migration_plans p
        WHERE p.state IN ('proposed', 'approved', 'running')
        UNION ALL
        SELECT coalesce(octet_length(i.resolution::text), 0)
             + coalesce(octet_length(i.problem::text), 0)
        FROM public.migration_items i
        JOIN public.migration_plans p
          ON p.project_id = i.project_id AND p.environment_id = i.environment_id
         AND p.id = i.plan_id
        WHERE p.state IN ('proposed', 'approved', 'running')
          AND (i.resolution IS NOT NULL OR i.problem IS NOT NULL)
    ) pending
$$;

-- Records one statement's change, if it changed anything.
CREATE FUNCTION colophon_payload_note(delta bigint)
    RETURNS void
    LANGUAGE sql
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
    INSERT INTO public.payload_deltas (bytes) SELECT delta WHERE delta <> 0
$$;

-- One function per table, each reading only the transition tables its event has. Every one
-- runs as colophon_worker, the role that sees every project's work rows (0050), whichever role
-- wrote: a bulk item's weight depends on its job's state, an asset job's on its asset's size.

CREATE FUNCTION colophon_payload_jobs() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    plus bigint := 0;
    minus bigint := 0;
BEGIN
    IF TG_OP IN ('INSERT', 'UPDATE') THEN
        SELECT coalesce(sum(octet_length(idempotency_key)), 0) INTO plus FROM added
        WHERE status IN ('pending', 'retrying', 'running', 'held');
    END IF;
    IF TG_OP IN ('DELETE', 'UPDATE') THEN
        SELECT coalesce(sum(octet_length(idempotency_key)), 0) INTO minus FROM removed
        WHERE status IN ('pending', 'retrying', 'running', 'held');
    END IF;
    PERFORM public.colophon_payload_note((plus - minus)::bigint);
    RETURN NULL;
END
$$;

CREATE FUNCTION colophon_payload_asset_jobs() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    plus bigint := 0;
    minus bigint := 0;
BEGIN
    IF TG_OP IN ('INSERT', 'UPDATE') THEN
        SELECT coalesce(sum(octet_length(j.idempotency_key) + a.size_bytes), 0) INTO plus
        FROM added j JOIN public.assets a
          ON a.project_id = j.project_id AND a.environment_id = j.environment_id AND a.id = j.asset_id
        WHERE j.status IN ('pending', 'running', 'retrying');
    END IF;
    IF TG_OP IN ('DELETE', 'UPDATE') THEN
        SELECT coalesce(sum(octet_length(j.idempotency_key) + a.size_bytes), 0) INTO minus
        FROM removed j JOIN public.assets a
          ON a.project_id = j.project_id AND a.environment_id = j.environment_id AND a.id = j.asset_id
        WHERE j.status IN ('pending', 'running', 'retrying');
    END IF;
    PERFORM public.colophon_payload_note((plus - minus)::bigint);
    RETURN NULL;
END
$$;

-- An asset's recorded size moves the weight of each of its open jobs.
CREATE FUNCTION colophon_payload_assets() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    PERFORM public.colophon_payload_note(((
        SELECT coalesce(sum(
                   coalesce(octet_length(j.idempotency_key) + n.size_bytes, 0)
                 - coalesce(octet_length(j.idempotency_key) + o.size_bytes, 0)), 0)
        FROM added n
        JOIN removed o
          ON o.project_id = n.project_id AND o.environment_id = n.environment_id AND o.id = n.id
        JOIN public.asset_jobs j
          ON j.project_id = n.project_id AND j.environment_id = n.environment_id AND j.asset_id = n.id
        WHERE j.status IN ('pending', 'running', 'retrying')
          AND n.size_bytes IS DISTINCT FROM o.size_bytes))::bigint);
    RETURN NULL;
END
$$;

-- A deleted asset takes its open jobs' weight with it, counted while they can still be seen.
CREATE FUNCTION colophon_payload_asset_gone() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    PERFORM public.colophon_payload_note((-(
        SELECT coalesce(sum(octet_length(j.idempotency_key) + OLD.size_bytes), 0)
        FROM public.asset_jobs j
        WHERE j.project_id = OLD.project_id AND j.environment_id = OLD.environment_id
          AND j.asset_id = OLD.id AND j.status IN ('pending', 'running', 'retrying')))::bigint);
    RETURN OLD;
END
$$;

-- A bulk job's own operation, and its pending items' inputs when it opens or closes.
CREATE FUNCTION colophon_payload_bulk_jobs() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    plus bigint := 0;
    minus bigint := 0;
    carried bigint := 0;
BEGIN
    IF TG_OP IN ('INSERT', 'UPDATE') THEN
        SELECT coalesce(sum(octet_length(operation::text)), 0) INTO plus FROM added
        WHERE state IN ('accepting', 'running');
    END IF;
    IF TG_OP = 'UPDATE' THEN
        SELECT coalesce(sum(octet_length(operation::text)), 0) INTO minus FROM removed
        WHERE state IN ('accepting', 'running');
        -- Jobs that opened or closed carry their pending items in or out.
        SELECT coalesce(sum(CASE WHEN n.state IN ('accepting', 'running') THEN 1 ELSE -1 END
                            * (SELECT coalesce(sum(octet_length(i.input::text)), 0)
                               FROM public.bulk_items i
                               WHERE i.project_id = n.project_id AND i.environment_id = n.environment_id
                                 AND i.job_id = n.id AND i.outcome = 'pending')), 0)
        INTO carried
        FROM added n
        JOIN removed o
          ON o.project_id = n.project_id AND o.environment_id = n.environment_id AND o.id = n.id
        WHERE (n.state IN ('accepting', 'running')) <> (o.state IN ('accepting', 'running'));
    END IF;
    PERFORM public.colophon_payload_note((plus - minus + carried)::bigint);
    RETURN NULL;
END
$$;

-- A deleted bulk job: its own operation and its pending items, while they can be seen.
CREATE FUNCTION colophon_payload_bulk_job_gone() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    IF OLD.state IN ('accepting', 'running') THEN
        PERFORM public.colophon_payload_note((-(
            coalesce(octet_length(OLD.operation::text), 0)
            + (SELECT coalesce(sum(octet_length(i.input::text)), 0) FROM public.bulk_items i
               WHERE i.project_id = OLD.project_id AND i.environment_id = OLD.environment_id
                 AND i.job_id = OLD.id AND i.outcome = 'pending')))::bigint);
    END IF;
    RETURN OLD;
END
$$;

-- A bulk item weighs its input while it is pending and its job is open.
CREATE FUNCTION colophon_payload_bulk_items() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    plus bigint := 0;
    minus bigint := 0;
BEGIN
    IF TG_OP IN ('INSERT', 'UPDATE') THEN
        SELECT coalesce(sum(octet_length(i.input::text)), 0) INTO plus
        FROM added i JOIN public.bulk_jobs b
          ON b.project_id = i.project_id AND b.environment_id = i.environment_id AND b.id = i.job_id
        WHERE b.state IN ('accepting', 'running') AND i.outcome = 'pending';
    END IF;
    IF TG_OP IN ('DELETE', 'UPDATE') THEN
        SELECT coalesce(sum(octet_length(i.input::text)), 0) INTO minus
        FROM removed i JOIN public.bulk_jobs b
          ON b.project_id = i.project_id AND b.environment_id = i.environment_id AND b.id = i.job_id
        WHERE b.state IN ('accepting', 'running') AND i.outcome = 'pending';
    END IF;
    PERFORM public.colophon_payload_note((plus - minus)::bigint);
    RETURN NULL;
END
$$;

-- A plan's own definition and operations, and its items' stored bytes when it opens or closes.
CREATE FUNCTION colophon_payload_plans() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    plus bigint := 0;
    minus bigint := 0;
    carried bigint := 0;
BEGIN
    IF TG_OP IN ('INSERT', 'UPDATE') THEN
        SELECT coalesce(sum(octet_length(target_definition::text) + octet_length(operations::text)), 0)
        INTO plus FROM added WHERE state IN ('proposed', 'approved', 'running');
    END IF;
    IF TG_OP = 'UPDATE' THEN
        SELECT coalesce(sum(octet_length(target_definition::text) + octet_length(operations::text)), 0)
        INTO minus FROM removed WHERE state IN ('proposed', 'approved', 'running');
        SELECT coalesce(sum(CASE WHEN n.state IN ('proposed', 'approved', 'running') THEN 1 ELSE -1 END
                            * (SELECT coalesce(sum(coalesce(octet_length(i.resolution::text), 0)
                                                 + coalesce(octet_length(i.problem::text), 0)), 0)
                               FROM public.migration_items i
                               WHERE i.project_id = n.project_id AND i.environment_id = n.environment_id
                                 AND i.plan_id = n.id)), 0)
        INTO carried
        FROM added n
        JOIN removed o
          ON o.project_id = n.project_id AND o.environment_id = n.environment_id AND o.id = n.id
        WHERE (n.state IN ('proposed', 'approved', 'running'))
           <> (o.state IN ('proposed', 'approved', 'running'));
    END IF;
    PERFORM public.colophon_payload_note((plus - minus + carried)::bigint);
    RETURN NULL;
END
$$;

CREATE FUNCTION colophon_payload_plan_gone() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    IF OLD.state IN ('proposed', 'approved', 'running') THEN
        PERFORM public.colophon_payload_note((-(
            coalesce(octet_length(OLD.target_definition::text) + octet_length(OLD.operations::text), 0)
            + (SELECT coalesce(sum(coalesce(octet_length(i.resolution::text), 0)
                                 + coalesce(octet_length(i.problem::text), 0)), 0)
               FROM public.migration_items i
               WHERE i.project_id = OLD.project_id AND i.environment_id = OLD.environment_id
                 AND i.plan_id = OLD.id)))::bigint);
    END IF;
    RETURN OLD;
END
$$;

-- A plan item weighs its stored resolution and problem while its plan is open.
CREATE FUNCTION colophon_payload_plan_items() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    plus bigint := 0;
    minus bigint := 0;
BEGIN
    IF TG_OP IN ('INSERT', 'UPDATE') THEN
        SELECT coalesce(sum(coalesce(octet_length(i.resolution::text), 0)
                          + coalesce(octet_length(i.problem::text), 0)), 0) INTO plus
        FROM added i JOIN public.migration_plans p
          ON p.project_id = i.project_id AND p.environment_id = i.environment_id AND p.id = i.plan_id
        WHERE p.state IN ('proposed', 'approved', 'running');
    END IF;
    IF TG_OP IN ('DELETE', 'UPDATE') THEN
        SELECT coalesce(sum(coalesce(octet_length(i.resolution::text), 0)
                          + coalesce(octet_length(i.problem::text), 0)), 0) INTO minus
        FROM removed i JOIN public.migration_plans p
          ON p.project_id = i.project_id AND p.environment_id = i.environment_id AND p.id = i.plan_id
        WHERE p.state IN ('proposed', 'approved', 'running');
    END IF;
    PERFORM public.colophon_payload_note((plus - minus)::bigint);
    RETURN NULL;
END
$$;

-- Writes wait while the triggers go in and the total is counted once, so no change can fall
-- between the count and the first trigger (reads go on).
LOCK TABLE jobs, asset_jobs, assets, bulk_jobs, bulk_items, migration_plans, migration_items
    IN SHARE ROW EXCLUSIVE MODE;

DO $$
DECLARE
    source record;
BEGIN
    FOR source IN
        SELECT * FROM (VALUES
            ('jobs', 'colophon_payload_jobs'),
            ('asset_jobs', 'colophon_payload_asset_jobs'),
            ('bulk_jobs', 'colophon_payload_bulk_jobs'),
            ('bulk_items', 'colophon_payload_bulk_items'),
            ('migration_plans', 'colophon_payload_plans'),
            ('migration_items', 'colophon_payload_plan_items')
        ) AS t (tbl, fn)
    LOOP
        EXECUTE format(
            'CREATE TRIGGER %I AFTER INSERT ON %I REFERENCING NEW TABLE AS added
                 FOR EACH STATEMENT EXECUTE FUNCTION %I()',
            source.tbl || '_payload_insert', source.tbl, source.fn);
        EXECUTE format(
            'CREATE TRIGGER %I AFTER UPDATE ON %I REFERENCING OLD TABLE AS removed NEW TABLE AS added
                 FOR EACH STATEMENT EXECUTE FUNCTION %I()',
            source.tbl || '_payload_update', source.tbl, source.fn);
        -- A parent's deletion is counted before it happens (below), children and all.
        IF source.tbl NOT IN ('bulk_jobs', 'migration_plans') THEN
            EXECUTE format(
                'CREATE TRIGGER %I AFTER DELETE ON %I REFERENCING OLD TABLE AS removed
                     FOR EACH STATEMENT EXECUTE FUNCTION %I()',
                source.tbl || '_payload_delete', source.tbl, source.fn);
        END IF;
    END LOOP;
END
$$;
CREATE TRIGGER assets_payload_update AFTER UPDATE ON assets
    REFERENCING OLD TABLE AS removed NEW TABLE AS added
    FOR EACH STATEMENT EXECUTE FUNCTION colophon_payload_assets();
-- Named to run after any other BEFORE DELETE trigger, so a delete that is skipped is never
-- counted.
CREATE TRIGGER zz_assets_payload_delete BEFORE DELETE ON assets
    FOR EACH ROW EXECUTE FUNCTION colophon_payload_asset_gone();
CREATE TRIGGER zz_bulk_jobs_payload_delete BEFORE DELETE ON bulk_jobs
    FOR EACH ROW EXECUTE FUNCTION colophon_payload_bulk_job_gone();
CREATE TRIGGER zz_migration_plans_payload_delete BEFORE DELETE ON migration_plans
    FOR EACH ROW EXECUTE FUNCTION colophon_payload_plan_gone();

INSERT INTO payload_counter (id, bytes) VALUES (1, colophon_payload_actual());

-- An admission's check, at the end of its transaction: the counter row locked to commit, every
-- visible delta folded into it (this transaction's own included), the total returned.
CREATE FUNCTION colophon_payload_admit()
    RETURNS bigint
    LANGUAGE plpgsql
    VOLATILE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    total bigint;
BEGIN
    PERFORM 1 FROM public.payload_counter WHERE id = 1 FOR UPDATE;
    WITH moved AS (DELETE FROM public.payload_deltas RETURNING bytes)
    UPDATE public.payload_counter
       SET bytes = bytes + (SELECT coalesce(sum(bytes), 0) FROM moved)::bigint
     WHERE id = 1
    RETURNING bytes INTO total;
    RETURN total;
END
$$;

-- The total without a lock: the folded count plus the deltas this transaction can see. An
-- admission's early refusal and the worker's budgeting read it; it may lag other transactions'
-- uncommitted work, which the admission's own check at commit does not.
CREATE FUNCTION colophon_payload_total()
    RETURNS bigint
    LANGUAGE sql
    STABLE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
    SELECT ((SELECT bytes FROM public.payload_counter WHERE id = 1)
          + (SELECT coalesce(sum(bytes), 0) FROM public.payload_deltas))::bigint
$$;

-- The worker's fold: the deltas folded into the counter, unless an admission holds it (then
-- this turn is skipped: the admission folds them). Returns how many deltas it folded, or -1.
CREATE FUNCTION colophon_payload_tick()
    RETURNS bigint
    LANGUAGE plpgsql
    VOLATILE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    folded bigint;
BEGIN
    PERFORM 1 FROM public.payload_counter WHERE id = 1 FOR UPDATE SKIP LOCKED;
    IF NOT FOUND THEN
        RETURN -1;
    END IF;
    WITH moved AS (DELETE FROM public.payload_deltas RETURNING bytes),
         applied AS (
             UPDATE public.payload_counter
                SET bytes = bytes + (SELECT coalesce(sum(bytes), 0) FROM moved)::bigint
              WHERE id = 1)
    SELECT count(*) INTO folded FROM moved;
    RETURN folded;
END
$$;

-- The running total against the SUM it stands for: drift is reported, never repaired.
CREATE FUNCTION colophon_payload_reconcile(OUT counted bigint, OUT actual bigint, OUT drift bigint)
    LANGUAGE sql
    STABLE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
    SELECT t.counted, t.actual, t.counted - t.actual
    FROM (SELECT public.colophon_payload_total() AS counted,
                 public.colophon_payload_actual() AS actual) t
$$;

-- How long the log is, and how old its oldest delta, for the metrics surface.
CREATE FUNCTION colophon_payload_log(OUT deltas bigint, OUT oldest_seconds double precision)
    LANGUAGE sql
    STABLE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
    SELECT count(*), coalesce(extract(epoch FROM now() - min(at)), 0)::double precision
    FROM public.payload_deltas
$$;

ALTER TABLE payload_counter OWNER TO colophon_worker;
ALTER TABLE payload_deltas OWNER TO colophon_worker;
DO $$
DECLARE
    fn text;
BEGIN
    FOREACH fn IN ARRAY ARRAY[
        'colophon_payload_actual()', 'colophon_payload_note(bigint)',
        'colophon_payload_jobs()', 'colophon_payload_asset_jobs()', 'colophon_payload_assets()',
        'colophon_payload_asset_gone()', 'colophon_payload_bulk_jobs()',
        'colophon_payload_bulk_job_gone()', 'colophon_payload_bulk_items()',
        'colophon_payload_plans()', 'colophon_payload_plan_gone()',
        'colophon_payload_plan_items()', 'colophon_payload_admit()', 'colophon_payload_total()',
        'colophon_payload_tick()', 'colophon_payload_reconcile()', 'colophon_payload_log()']
    LOOP
        EXECUTE format('ALTER FUNCTION %s OWNER TO colophon_worker', fn);
        EXECUTE format('REVOKE ALL ON FUNCTION %s FROM PUBLIC', fn);
    END LOOP;
END
$$;
GRANT EXECUTE ON FUNCTION colophon_payload_admit(), colophon_payload_total(),
    colophon_payload_reconcile(), colophon_payload_log() TO colophon_api, colophon_worker;
GRANT EXECUTE ON FUNCTION colophon_payload_tick() TO colophon_worker;

COMMENT ON FUNCTION colophon_pending_payload() IS
    'Superseded by colophon_payload_admit/total (D-216); kept for a server one release older during an upgrade.';
