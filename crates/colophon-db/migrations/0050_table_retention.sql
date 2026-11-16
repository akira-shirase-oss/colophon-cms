-- Milestone P6: bounded tables (D-135, spec §27, §33; docs/modules/02-db.md "Retention",
-- docs/modules/05-worker.md "Retention", docs/modules/18-operations.md "Retention").
--
-- Until now the only bounded sweep was media's: every other table that accrues rows grew
-- without a delete path. This migration gives the worker's table-retention sweep its
-- privileges and its indexes, and the audit log its one sanctioned delete. The sweep
-- itself is `colophon_db::retention::sweep`; the ages live there, spelled out per table.
--
-- Two pieces of the design are structural and live here:
--
-- 1. The audit log's append-only guarantee stays a trigger: `colophon_audit_append_only`
--    lets a DELETE through while `colophon.audit_sweep` is set for the transaction — a
--    flag nothing but `colophon_audit_sweep` sets usefully, since the function runs as
--    `colophon_retention`, a role that can delete audit rows and nothing else (the
--    colophon_deleter pattern, 0030) — or while `app.deleting_project` names the very
--    project the row belongs to, `colophon_delete_project`'s flag (0048). A worker or an
--    API transaction can set either flag; neither holds the DELETE it would open.
--
-- 2. `colophon_pending_payload` counts the installation's pending payload bytes for the
--    §33 "1 GiB pending payload metadata per installation" bound (D-135). It runs as
--    `colophon_worker`, the one role that already sees every project's work rows — an
--    application transaction's own project scope could never see the other projects'
--    payload it is meant to be bounded against. It takes the byte-admission lock itself
--    before counting, so a caller always gets a count made under the lock, and sees its
--    own uncommitted writes like any query in its transaction.

-- The audit sweep's owner: a role that can do exactly one thing.
DO $$
BEGIN
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'colophon_retention') THEN
        CREATE ROLE colophon_retention NOLOGIN NOSUPERUSER NOBYPASSRLS;
    END IF;
END
$$;

CREATE POLICY audit_events_for_retention ON audit_events FOR ALL TO colophon_retention
    USING (true) WITH CHECK (true);
GRANT SELECT, DELETE ON audit_events TO colophon_retention;

-- The append-only rule, with its two sanctioned passes. The flags are read with
-- `current_setting(..., true)`: unset they answer NULL, never an error — and holding one
-- opens nothing for a role without the DELETE, which only colophon_retention and
-- colophon_deleter hold.
CREATE OR REPLACE FUNCTION colophon_audit_append_only() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    -- The retention sweep: a bounded delete of events past their age.
    IF tg_op = 'DELETE' AND current_setting('colophon.audit_sweep', true) = 'on' THEN
        RETURN OLD;
    END IF;
    -- A project's deletion: its trail goes with it — the ledger keeps the record off-host
    -- (0048), and the flag names the very project, never another's rows. `nullif` keeps an
    -- unset or empty flag from raising on the cast, as 0048's own checks read it.
    IF tg_op = 'DELETE'
       AND nullif(current_setting('app.deleting_project', true), '')::uuid = OLD.project_id
    THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'the audit log is append-only: % refused', tg_op
        USING ERRCODE = 'insufficient_privilege';
END
$$;

-- The one delete the audit log allows: oldest events first, at most `batch`, only those
-- older than `cutoff`. SECURITY DEFINER owned by colophon_retention, so the flag it sets
-- and the rows it touches stay inside the role built for this and nothing else.
CREATE FUNCTION colophon_audit_sweep(cutoff timestamptz, batch bigint)
    RETURNS bigint
    LANGUAGE plpgsql
    VOLATILE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    removed bigint;
BEGIN
    PERFORM set_config('colophon.audit_sweep', 'on', true);
    WITH doomed AS (
        SELECT project_id, id FROM public.audit_events
        WHERE at < cutoff
        ORDER BY at
        LIMIT batch)
    DELETE FROM public.audit_events e USING doomed d
        WHERE e.project_id = d.project_id AND e.id = d.id;
    GET DIAGNOSTICS removed = ROW_COUNT;
    RETURN removed;
END
$$;
ALTER FUNCTION colophon_audit_sweep(timestamptz, bigint) OWNER TO colophon_retention;
REVOKE ALL ON FUNCTION colophon_audit_sweep(timestamptz, bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_audit_sweep(timestamptz, bigint) TO colophon_worker;

-- The installation's pending payload bytes (spec §33's second bound, D-135): the stored
-- variable-size bytes of every open unit of work, however small each is — a delivery job's
-- idempotency key; an asset job's key plus its asset's recorded object bytes; a bulk job's
-- operation document plus its still-pending items' inputs; a migration plan's target
-- definition and operations plus its items' stored resolutions. Object bytes live in
-- storage, not here; `assets.size_bytes` is the recorded, trusted count of them, so an
-- upload's true cost is what admission weighs — there is nothing else honest to weigh.
-- pg_advisory_xact_lock class 0x5742 (22338, "WB") is the byte-admission lock: every payload
-- counting or admitting transaction holds it before its first row lock, so two admissions
-- can never both see room that isn't there, and no lock cycle can form (the project
-- admission lock 0x5741 always comes first — WA before WB, everywhere).
CREATE FUNCTION colophon_pending_payload()
    RETURNS bigint
    LANGUAGE plpgsql
    VOLATILE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    PERFORM pg_advisory_xact_lock(22338, 0);
    RETURN (
        SELECT coalesce(sum(bytes), 0) FROM (
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
        ) pending);
END
$$;
ALTER FUNCTION colophon_pending_payload() OWNER TO colophon_worker;
REVOKE ALL ON FUNCTION colophon_pending_payload() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_pending_payload() TO colophon_api;

-- The sweep's reach on the work tables: DELETE on finished history, over the SELECT the
-- worker already has — its FOR ALL policies already exist, and its doomed sets read only
-- the columns its claims and receipts already read. The one gap is `finished_at` on
-- `schedules`, where the worker's SELECT is column-scoped.
GRANT DELETE ON jobs, observations, asset_jobs, bulk_jobs, bulk_items,
                migration_plans, migration_items, budget_reservations
    TO colophon_worker;
GRANT SELECT (finished_at), DELETE ON schedules TO colophon_worker;

-- The session and flow tables are different: they are private to the sign-in and invite
-- flows, the worker has never been able to read them, and the jobs test proves it stays
-- that way. Their deletes go through the same door the audit log's does — functions owned
-- by `colophon_retention`, executed by a worker that can never see the rows it removes.
-- `idempotency_records` is FORCE row-security, so the owner role needs a policy before
-- the grants mean anything; the rest have no RLS at all.
CREATE POLICY idempotency_records_for_retention ON idempotency_records
    FOR ALL TO colophon_retention USING (true) WITH CHECK (true);
GRANT SELECT, DELETE ON idempotency_records, sessions, access_tokens, invitations,
                        sign_in_attempts, device_logins, setup_codes, setup_attempts
    TO colophon_retention;

-- Each takes the cutoff its age implies and a batch, deletes oldest first, and answers
-- how many it removed — the same shape as `colophon_audit_sweep` above.
CREATE FUNCTION colophon_sweep_sessions(cutoff timestamptz, batch bigint)
    RETURNS bigint LANGUAGE plpgsql VOLATILE SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE removed bigint;
BEGIN
    WITH doomed AS (
        SELECT id FROM public.sessions
        WHERE expires_at < cutoff ORDER BY expires_at LIMIT batch)
    DELETE FROM public.sessions s USING doomed d WHERE s.id = d.id;
    GET DIAGNOSTICS removed = ROW_COUNT;
    RETURN removed;
END $$;

CREATE FUNCTION colophon_sweep_access_tokens(cutoff timestamptz, batch bigint)
    RETURNS bigint LANGUAGE plpgsql VOLATILE SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE removed bigint;
BEGIN
    WITH doomed AS (
        SELECT id FROM public.access_tokens
        WHERE expires_at < cutoff ORDER BY expires_at LIMIT batch)
    DELETE FROM public.access_tokens t USING doomed d WHERE t.id = d.id;
    GET DIAGNOSTICS removed = ROW_COUNT;
    RETURN removed;
END $$;

-- An invitation nobody accepted — lapsed, or revoked — that is itself old. An accepted
-- one stays: its membership is the record.
CREATE FUNCTION colophon_sweep_invitations(cutoff timestamptz, batch bigint)
    RETURNS bigint LANGUAGE plpgsql VOLATILE SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE removed bigint;
BEGIN
    WITH doomed AS (
        SELECT id FROM public.invitations
        WHERE accepted_at IS NULL AND (expires_at < cutoff OR revoked_at < cutoff)
        ORDER BY created_at LIMIT batch)
    DELETE FROM public.invitations i USING doomed d WHERE i.id = d.id;
    GET DIAGNOSTICS removed = ROW_COUNT;
    RETURN removed;
END $$;

-- Dead device and sign-in flows: their writers prune the same rows on a busy day; the
-- sweep keeps a quiet one bounded too. `now` is the caller's, so a test controls it.
CREATE FUNCTION colophon_sweep_device_logins(now timestamptz, batch bigint)
    RETURNS bigint LANGUAGE plpgsql VOLATILE SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE removed bigint;
BEGIN
    WITH doomed AS (
        SELECT device_hash FROM public.device_logins
        WHERE expires_at < now ORDER BY expires_at LIMIT batch)
    DELETE FROM public.device_logins d USING doomed x WHERE d.device_hash = x.device_hash;
    GET DIAGNOSTICS removed = ROW_COUNT;
    RETURN removed;
END $$;

CREATE FUNCTION colophon_sweep_sign_in_attempts(now timestamptz, batch bigint)
    RETURNS bigint LANGUAGE plpgsql VOLATILE SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE removed bigint;
BEGIN
    WITH doomed AS (
        SELECT state_hash FROM public.sign_in_attempts
        WHERE expires_at < now ORDER BY expires_at LIMIT batch)
    DELETE FROM public.sign_in_attempts a USING doomed d WHERE a.state_hash = d.state_hash;
    GET DIAGNOSTICS removed = ROW_COUNT;
    RETURN removed;
END $$;

CREATE FUNCTION colophon_sweep_setup_attempts(cutoff timestamptz, batch bigint)
    RETURNS bigint LANGUAGE plpgsql VOLATILE SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE removed bigint;
BEGIN
    WITH doomed AS (
        SELECT id FROM public.setup_attempts
        WHERE attempted_at < cutoff ORDER BY attempted_at LIMIT batch)
    DELETE FROM public.setup_attempts a USING doomed d WHERE a.id = d.id;
    GET DIAGNOSTICS removed = ROW_COUNT;
    RETURN removed;
END $$;

-- A used code's administrator is the record; an expired one is dead. Both go past the
-- flow age.
CREATE FUNCTION colophon_sweep_setup_codes(cutoff timestamptz, batch bigint)
    RETURNS bigint LANGUAGE plpgsql VOLATILE SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE removed bigint;
BEGIN
    WITH doomed AS (
        SELECT id FROM public.setup_codes
        WHERE expires_at < cutoff ORDER BY expires_at LIMIT batch)
    DELETE FROM public.setup_codes c USING doomed d WHERE c.id = d.id;
    GET DIAGNOSTICS removed = ROW_COUNT;
    RETURN removed;
END $$;

CREATE FUNCTION colophon_sweep_idempotency(cutoff timestamptz, batch bigint)
    RETURNS bigint LANGUAGE plpgsql VOLATILE SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE removed bigint;
BEGIN
    WITH doomed AS (
        SELECT project_id, environment_id, identity_id, operation, key
        FROM public.idempotency_records
        WHERE created_at < cutoff ORDER BY created_at LIMIT batch)
    DELETE FROM public.idempotency_records r USING doomed d
        WHERE r.project_id = d.project_id AND r.environment_id = d.environment_id
          AND r.identity_id = d.identity_id AND r.operation = d.operation
          AND r.key = d.key;
    GET DIAGNOSTICS removed = ROW_COUNT;
    RETURN removed;
END $$;

ALTER FUNCTION colophon_sweep_sessions(timestamptz, bigint) OWNER TO colophon_retention;
ALTER FUNCTION colophon_sweep_access_tokens(timestamptz, bigint) OWNER TO colophon_retention;
ALTER FUNCTION colophon_sweep_invitations(timestamptz, bigint) OWNER TO colophon_retention;
ALTER FUNCTION colophon_sweep_device_logins(timestamptz, bigint) OWNER TO colophon_retention;
ALTER FUNCTION colophon_sweep_sign_in_attempts(timestamptz, bigint) OWNER TO colophon_retention;
ALTER FUNCTION colophon_sweep_setup_attempts(timestamptz, bigint) OWNER TO colophon_retention;
ALTER FUNCTION colophon_sweep_setup_codes(timestamptz, bigint) OWNER TO colophon_retention;
ALTER FUNCTION colophon_sweep_idempotency(timestamptz, bigint) OWNER TO colophon_retention;
REVOKE ALL ON FUNCTION colophon_sweep_sessions(timestamptz, bigint) FROM PUBLIC;
REVOKE ALL ON FUNCTION colophon_sweep_access_tokens(timestamptz, bigint) FROM PUBLIC;
REVOKE ALL ON FUNCTION colophon_sweep_invitations(timestamptz, bigint) FROM PUBLIC;
REVOKE ALL ON FUNCTION colophon_sweep_device_logins(timestamptz, bigint) FROM PUBLIC;
REVOKE ALL ON FUNCTION colophon_sweep_sign_in_attempts(timestamptz, bigint) FROM PUBLIC;
REVOKE ALL ON FUNCTION colophon_sweep_setup_attempts(timestamptz, bigint) FROM PUBLIC;
REVOKE ALL ON FUNCTION colophon_sweep_setup_codes(timestamptz, bigint) FROM PUBLIC;
REVOKE ALL ON FUNCTION colophon_sweep_idempotency(timestamptz, bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_sweep_sessions(timestamptz, bigint) TO colophon_worker;
GRANT EXECUTE ON FUNCTION colophon_sweep_access_tokens(timestamptz, bigint) TO colophon_worker;
GRANT EXECUTE ON FUNCTION colophon_sweep_invitations(timestamptz, bigint) TO colophon_worker;
GRANT EXECUTE ON FUNCTION colophon_sweep_device_logins(timestamptz, bigint) TO colophon_worker;
GRANT EXECUTE ON FUNCTION colophon_sweep_sign_in_attempts(timestamptz, bigint) TO colophon_worker;
GRANT EXECUTE ON FUNCTION colophon_sweep_setup_attempts(timestamptz, bigint) TO colophon_worker;
GRANT EXECUTE ON FUNCTION colophon_sweep_setup_codes(timestamptz, bigint) TO colophon_worker;
GRANT EXECUTE ON FUNCTION colophon_sweep_idempotency(timestamptz, bigint) TO colophon_worker;

-- What the sweep's doomed-set scans lean on (the expiry indexes on sign_in_attempts,
-- device_logins, idempotency_records and setup_attempts already exist).
CREATE INDEX jobs_retention ON jobs (finished_at) WHERE finished_at IS NOT NULL;
CREATE INDEX asset_jobs_retention ON asset_jobs (finished_at) WHERE finished_at IS NOT NULL;
CREATE INDEX bulk_jobs_retention ON bulk_jobs (finished_at) WHERE finished_at IS NOT NULL;
CREATE INDEX migration_plans_retention ON migration_plans (finished_at)
    WHERE finished_at IS NOT NULL;
CREATE INDEX schedules_retention ON schedules (finished_at) WHERE finished_at IS NOT NULL;
CREATE INDEX observations_by_age ON observations (observed_at);
CREATE INDEX audit_events_by_age ON audit_events (at);
CREATE INDEX sessions_by_expiry ON sessions (expires_at);
CREATE INDEX access_tokens_by_expiry ON access_tokens (expires_at);
CREATE INDEX invitations_by_expiry ON invitations (expires_at);
CREATE INDEX setup_codes_by_expiry ON setup_codes (expires_at);
CREATE INDEX budget_reservations_retention ON budget_reservations (settled_at)
    WHERE state IN ('settled', 'released');
