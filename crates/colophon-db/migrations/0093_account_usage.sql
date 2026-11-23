-- D-235: per-account counting across tenants, by colophon_accounts (0092).
--
-- colophon_projects_owned(account): how many projects the account owns, whether or not it is
-- still a member of them (a project an account left still counts against it). A definer
-- function: the caller can't see other tenants' projects, and mustn't.

CREATE FUNCTION colophon_projects_owned(account uuid) RETURNS integer
    LANGUAGE sql
    STABLE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
    SELECT count(*)::integer FROM public.projects p WHERE p.owner_account = account
$$;
ALTER FUNCTION colophon_projects_owned(uuid) OWNER TO colophon_accounts;
REVOKE ALL ON FUNCTION colophon_projects_owned(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_projects_owned(uuid) TO colophon_api;

-- ---------------------------------------------------------------------------------------------
-- Per-account quotas (D-235): what an account's projects hold, counted by statement triggers on
-- the tables that hold it, so every write path counts and is bounded the same way (the API, MCP,
-- bulk import, populate, the worker, the deleter and the retention sweeps), and project deletion
-- settles itself (colophon_delete_project deletes these rows before the project row).
--
-- account_usage:     entries, assets, stored_bytes (asset versions), pending_bytes (uploads still
--                    quarantined): one row per account that owns a project; written only by the
--                    definer functions below (owned by colophon_accounts), read by the API only
--                    through colophon_account_usage.
-- account_quotas:    the limits (0 = no bound), one row; the server writes it at boot from its
--                    environment (COLOPHON_ACCOUNT_MAX_*), so on several servers the last boot wins.
-- account_upload_reservations: the bytes of uploads admitted but not yet recorded as assets, so
--                    an upload is refused before its bytes reach the object store; an expired
--                    reservation simply stops counting.
-- Administrators' projects are counted but never refused.

CREATE TABLE account_usage (
    account uuid PRIMARY KEY REFERENCES identities (id),
    entries bigint NOT NULL DEFAULT 0 CHECK (entries >= 0),
    assets bigint NOT NULL DEFAULT 0 CHECK (assets >= 0),
    stored_bytes bigint NOT NULL DEFAULT 0 CHECK (stored_bytes >= 0),
    pending_bytes bigint NOT NULL DEFAULT 0 CHECK (pending_bytes >= 0)
);
ALTER TABLE account_usage ENABLE ROW LEVEL SECURITY;
ALTER TABLE account_usage FORCE ROW LEVEL SECURITY;
CREATE POLICY account_usage_by_accounts ON account_usage TO colophon_accounts USING (true) WITH CHECK (true);
GRANT SELECT, INSERT, UPDATE ON account_usage TO colophon_accounts;

CREATE TABLE account_quotas (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    entries bigint NOT NULL CHECK (entries >= 0),
    assets bigint NOT NULL CHECK (assets >= 0),
    storage_bytes bigint NOT NULL CHECK (storage_bytes >= 0),
    updated_at timestamptz NOT NULL DEFAULT now()
);
INSERT INTO account_quotas (entries, assets, storage_bytes) VALUES (10000, 2000, 1073741824);
ALTER TABLE account_quotas ENABLE ROW LEVEL SECURITY;
ALTER TABLE account_quotas FORCE ROW LEVEL SECURITY;
CREATE POLICY account_quotas_by_accounts ON account_quotas TO colophon_accounts USING (true) WITH CHECK (true);
GRANT SELECT, UPDATE ON account_quotas TO colophon_accounts;

CREATE TABLE account_upload_reservations (
    id uuid PRIMARY KEY,
    account uuid NOT NULL REFERENCES identities (id),
    bytes bigint NOT NULL CHECK (bytes > 0),
    expires_at timestamptz NOT NULL
);
CREATE INDEX account_upload_reservations_by_account ON account_upload_reservations (account, expires_at);
ALTER TABLE account_upload_reservations ENABLE ROW LEVEL SECURITY;
ALTER TABLE account_upload_reservations FORCE ROW LEVEL SECURITY;
CREATE POLICY account_upload_reservations_by_accounts ON account_upload_reservations TO colophon_accounts
    USING (true) WITH CHECK (true);
GRANT SELECT, INSERT, DELETE ON account_upload_reservations TO colophon_accounts;

-- What colophon_accounts reads to count and to reconcile: the counted columns, and who is an
-- administrator (exempt). Each of these tables forces row security, hence the policies.
GRANT SELECT (identity_id) ON administrators TO colophon_accounts;
GRANT SELECT (project_id) ON entries TO colophon_accounts;
GRANT SELECT (project_id, size_bytes, state) ON assets TO colophon_accounts;
GRANT SELECT (project_id, size_bytes) ON asset_versions TO colophon_accounts;
CREATE POLICY entries_counted_by_accounts ON entries FOR SELECT TO colophon_accounts USING (true);
CREATE POLICY assets_counted_by_accounts ON assets FOR SELECT TO colophon_accounts USING (true);
CREATE POLICY asset_versions_counted_by_accounts ON asset_versions FOR SELECT TO colophon_accounts USING (true);

-- colophon_usage_apply: adds the deltas to one account's row and, when told to enforce, refuses
-- (SQLSTATE CQ001, quota_exceeded) a positive delta that takes a quota past its limit. The check
-- reads what its own UPDATE produced (RETURNING): after waiting on another writer's lock, READ
-- COMMITTED re-reads the row, so the total is the committed one plus this statement's.
CREATE FUNCTION colophon_usage_apply(p_account uuid, d_entries bigint, d_assets bigint,
                                     d_stored bigint, d_pending bigint, enforce boolean)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    total record;
    q record;
BEGIN
    IF p_account IS NULL THEN
        RETURN;
    END IF;
    INSERT INTO public.account_usage AS u (account, entries, assets, stored_bytes, pending_bytes)
    VALUES (p_account, greatest(d_entries, 0), greatest(d_assets, 0), greatest(d_stored, 0), greatest(d_pending, 0))
    ON CONFLICT (account) DO UPDATE SET
        entries = greatest(u.entries + d_entries, 0),
        assets = greatest(u.assets + d_assets, 0),
        stored_bytes = greatest(u.stored_bytes + d_stored, 0),
        pending_bytes = greatest(u.pending_bytes + d_pending, 0)
    RETURNING u.entries, u.assets, u.stored_bytes + u.pending_bytes AS storage INTO total;
    IF NOT enforce OR EXISTS (SELECT FROM public.administrators a WHERE a.identity_id = p_account) THEN
        RETURN;
    END IF;
    SELECT * INTO q FROM public.account_quotas;
    IF d_entries > 0 AND q.entries > 0 AND total.entries > q.entries THEN
        RAISE EXCEPTION USING ERRCODE = 'CQ001', MESSAGE = 'quota_exceeded',
            DETAIL = json_build_object('quota', 'entries', 'limit', q.entries, 'used', total.entries - d_entries)::text;
    END IF;
    IF d_assets > 0 AND q.assets > 0 AND total.assets > q.assets THEN
        RAISE EXCEPTION USING ERRCODE = 'CQ001', MESSAGE = 'quota_exceeded',
            DETAIL = json_build_object('quota', 'assets', 'limit', q.assets, 'used', total.assets - d_assets)::text;
    END IF;
    IF (d_stored > 0 OR d_pending > 0) AND q.storage_bytes > 0 AND total.storage > q.storage_bytes THEN
        RAISE EXCEPTION USING ERRCODE = 'CQ001', MESSAGE = 'quota_exceeded',
            DETAIL = json_build_object('quota', 'storage_bytes', 'limit', q.storage_bytes,
                                       'used', total.storage - greatest(d_stored, 0) - greatest(d_pending, 0))::text;
    END IF;
END
$$;

-- The triggers: one row of deltas per owning account, applied in account order, so two
-- statements touching several accounts lock their rows in the same order and can't deadlock.
-- A row whose project is gone (or has no owner) counts against no one.
CREATE FUNCTION colophon_usage_entries() RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    d record;
BEGIN
    IF TG_OP = 'INSERT' THEN
        FOR d IN SELECT p.owner_account AS account, count(*) AS n
                 FROM new_rows r JOIN public.projects p ON p.id = r.project_id
                 WHERE p.owner_account IS NOT NULL GROUP BY 1 ORDER BY 1
        LOOP
            PERFORM public.colophon_usage_apply(d.account, (d.n)::bigint, (0)::bigint, (0)::bigint, (0)::bigint, true);
        END LOOP;
    ELSE
        FOR d IN SELECT p.owner_account AS account, count(*) AS n
                 FROM old_rows r JOIN public.projects p ON p.id = r.project_id
                 WHERE p.owner_account IS NOT NULL GROUP BY 1 ORDER BY 1
        LOOP
            PERFORM public.colophon_usage_apply(d.account, (-d.n)::bigint, (0)::bigint, (0)::bigint, (0)::bigint, false);
        END LOOP;
    END IF;
    RETURN NULL;
END
$$;

CREATE FUNCTION colophon_usage_assets() RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    d record;
BEGIN
    IF TG_OP = 'INSERT' THEN
        FOR d IN SELECT p.owner_account AS account, count(*) AS n,
                        coalesce(sum(r.size_bytes) FILTER (WHERE r.state = 'quarantined'), 0) AS pending
                 FROM new_rows r JOIN public.projects p ON p.id = r.project_id
                 WHERE p.owner_account IS NOT NULL GROUP BY 1 ORDER BY 1
        LOOP
            PERFORM public.colophon_usage_apply(d.account, (0)::bigint, (d.n)::bigint, (0)::bigint, (d.pending)::bigint, true);
        END LOOP;
    ELSIF TG_OP = 'DELETE' THEN
        FOR d IN SELECT p.owner_account AS account, count(*) AS n,
                        coalesce(sum(r.size_bytes) FILTER (WHERE r.state = 'quarantined'), 0) AS pending
                 FROM old_rows r JOIN public.projects p ON p.id = r.project_id
                 WHERE p.owner_account IS NOT NULL GROUP BY 1 ORDER BY 1
        LOOP
            PERFORM public.colophon_usage_apply(d.account, (0)::bigint, (-d.n)::bigint, (0)::bigint, (-d.pending)::bigint, false);
        END LOOP;
    ELSE
        -- An update moves bytes out of (or back into) quarantine: the net change per account.
        FOR d IN SELECT p.owner_account AS account,
                        coalesce(sum(n.size_bytes) FILTER (WHERE n.state = 'quarantined'), 0)
                        - coalesce(sum(o.size_bytes) FILTER (WHERE o.state = 'quarantined'), 0) AS pending
                 FROM new_rows n
                 JOIN old_rows o ON o.project_id = n.project_id AND o.environment_id = n.environment_id AND o.id = n.id
                 JOIN public.projects p ON p.id = n.project_id
                 WHERE p.owner_account IS NOT NULL GROUP BY 1 HAVING
                        coalesce(sum(n.size_bytes) FILTER (WHERE n.state = 'quarantined'), 0)
                        <> coalesce(sum(o.size_bytes) FILTER (WHERE o.state = 'quarantined'), 0)
                 ORDER BY 1
        LOOP
            PERFORM public.colophon_usage_apply(d.account, (0)::bigint, (0)::bigint, (0)::bigint, (d.pending)::bigint, d.pending > 0);
        END LOOP;
    END IF;
    RETURN NULL;
END
$$;

-- Asset versions are counted, not refused: a version is recorded at verification, possibly
-- before its upload leaves quarantine in the same transaction, and the bytes were bounded when
-- they arrived (the intake reservation and the assets trigger).
CREATE FUNCTION colophon_usage_asset_versions() RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    d record;
BEGIN
    IF TG_OP = 'INSERT' THEN
        FOR d IN SELECT p.owner_account AS account, sum(r.size_bytes) AS bytes
                 FROM new_rows r JOIN public.projects p ON p.id = r.project_id
                 WHERE p.owner_account IS NOT NULL GROUP BY 1 ORDER BY 1
        LOOP
            PERFORM public.colophon_usage_apply(d.account, (0)::bigint, (0)::bigint, (d.bytes)::bigint, (0)::bigint, false);
        END LOOP;
    ELSE
        FOR d IN SELECT p.owner_account AS account, sum(r.size_bytes) AS bytes
                 FROM old_rows r JOIN public.projects p ON p.id = r.project_id
                 WHERE p.owner_account IS NOT NULL GROUP BY 1 ORDER BY 1
        LOOP
            PERFORM public.colophon_usage_apply(d.account, (0)::bigint, (0)::bigint, (-d.bytes)::bigint, (0)::bigint, false);
        END LOOP;
    END IF;
    RETURN NULL;
END
$$;

CREATE TRIGGER entries_usage_insert AFTER INSERT ON entries REFERENCING NEW TABLE AS new_rows
    FOR EACH STATEMENT EXECUTE FUNCTION colophon_usage_entries();
CREATE TRIGGER entries_usage_delete AFTER DELETE ON entries REFERENCING OLD TABLE AS old_rows
    FOR EACH STATEMENT EXECUTE FUNCTION colophon_usage_entries();
CREATE TRIGGER assets_usage_insert AFTER INSERT ON assets REFERENCING NEW TABLE AS new_rows
    FOR EACH STATEMENT EXECUTE FUNCTION colophon_usage_assets();
CREATE TRIGGER assets_usage_delete AFTER DELETE ON assets REFERENCING OLD TABLE AS old_rows
    FOR EACH STATEMENT EXECUTE FUNCTION colophon_usage_assets();
CREATE TRIGGER assets_usage_update AFTER UPDATE ON assets REFERENCING OLD TABLE AS old_rows NEW TABLE AS new_rows
    FOR EACH STATEMENT EXECUTE FUNCTION colophon_usage_assets();
CREATE TRIGGER asset_versions_usage_insert AFTER INSERT ON asset_versions REFERENCING NEW TABLE AS new_rows
    FOR EACH STATEMENT EXECUTE FUNCTION colophon_usage_asset_versions();
CREATE TRIGGER asset_versions_usage_delete AFTER DELETE ON asset_versions REFERENCING OLD TABLE AS old_rows
    FOR EACH STATEMENT EXECUTE FUNCTION colophon_usage_asset_versions();

ALTER FUNCTION colophon_usage_apply(uuid, bigint, bigint, bigint, bigint, boolean) OWNER TO colophon_accounts;
ALTER FUNCTION colophon_usage_entries() OWNER TO colophon_accounts;
ALTER FUNCTION colophon_usage_assets() OWNER TO colophon_accounts;
ALTER FUNCTION colophon_usage_asset_versions() OWNER TO colophon_accounts;
REVOKE ALL ON FUNCTION colophon_usage_apply(uuid, bigint, bigint, bigint, bigint, boolean) FROM PUBLIC;
REVOKE ALL ON FUNCTION colophon_usage_entries() FROM PUBLIC;
REVOKE ALL ON FUNCTION colophon_usage_assets() FROM PUBLIC;
REVOKE ALL ON FUNCTION colophon_usage_asset_versions() FROM PUBLIC;

-- The account that owns the entered project (app.project_id, which only the entry functions
-- issue): the reservation functions act for that account alone, never one a caller names.
CREATE FUNCTION colophon_usage_entered_account() RETURNS uuid
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
    SELECT p.owner_account FROM public.projects p
    WHERE p.id = nullif(current_setting('app.project_id', true), '')::uuid
$$;
ALTER FUNCTION colophon_usage_entered_account() OWNER TO colophon_accounts;
REVOKE ALL ON FUNCTION colophon_usage_entered_account() FROM PUBLIC;

-- Upload intake: before an upload's bytes reach the object store, reserve them against the
-- entered project's account, under that account's usage row lock: stored + quarantined + live
-- reservations + these bytes must fit the storage limit, or CQ001. The reservation is removed
-- when the upload is recorded (the assets trigger then counts its bytes as pending) and simply
-- stops counting once it expires (a failed or abandoned upload).
CREATE FUNCTION colophon_usage_reserve(p_reservation uuid, p_bytes bigint, p_seconds integer)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_account uuid := public.colophon_usage_entered_account();
    v_held bigint;
    v_reserved bigint;
    v_limit bigint;
BEGIN
    IF v_account IS NULL OR p_bytes <= 0
       OR EXISTS (SELECT FROM public.administrators a WHERE a.identity_id = v_account) THEN
        RETURN;
    END IF;
    INSERT INTO public.account_usage (account) VALUES (v_account) ON CONFLICT (account) DO NOTHING;
    SELECT u.stored_bytes + u.pending_bytes INTO v_held
    FROM public.account_usage u WHERE u.account = v_account FOR UPDATE;
    DELETE FROM public.account_upload_reservations r WHERE r.account = v_account AND r.expires_at <= now();
    SELECT coalesce(sum(r.bytes), 0) INTO v_reserved
    FROM public.account_upload_reservations r WHERE r.account = v_account;
    SELECT q.storage_bytes INTO v_limit FROM public.account_quotas q;
    IF v_limit > 0 AND v_held + v_reserved + p_bytes > v_limit THEN
        RAISE EXCEPTION USING ERRCODE = 'CQ001', MESSAGE = 'quota_exceeded',
            DETAIL = json_build_object('quota', 'storage_bytes', 'limit', v_limit, 'used', v_held + v_reserved)::text;
    END IF;
    INSERT INTO public.account_upload_reservations (id, account, bytes, expires_at)
    VALUES (p_reservation, v_account, p_bytes, now() + make_interval(secs => p_seconds));
END
$$;
ALTER FUNCTION colophon_usage_reserve(uuid, bigint, integer) OWNER TO colophon_accounts;
REVOKE ALL ON FUNCTION colophon_usage_reserve(uuid, bigint, integer) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_usage_reserve(uuid, bigint, integer) TO colophon_api;

CREATE FUNCTION colophon_usage_release(p_reservation uuid) RETURNS void
LANGUAGE sql
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
    DELETE FROM public.account_upload_reservations r
    WHERE r.id = p_reservation AND r.account = public.colophon_usage_entered_account()
$$;
ALTER FUNCTION colophon_usage_release(uuid) OWNER TO colophon_accounts;
REVOKE ALL ON FUNCTION colophon_usage_release(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_usage_release(uuid) TO colophon_api;

-- A replacement's reservation (its id is the verify job's) ends with the job: in the same
-- transaction that records the verdict (the new version landed, or the job failed or was
-- superseded) or removes the job, so a replaced asset is never counted twice, as its
-- reservation and as its new version. Expiry is only the fallback for a worker that died.
CREATE FUNCTION colophon_usage_job_settled() RETURNS trigger
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        DELETE FROM public.account_upload_reservations r WHERE r.id = OLD.id;
        RETURN OLD;
    END IF;
    DELETE FROM public.account_upload_reservations r WHERE r.id = NEW.id;
    RETURN NEW;
END
$$;
ALTER FUNCTION colophon_usage_job_settled() OWNER TO colophon_accounts;
REVOKE ALL ON FUNCTION colophon_usage_job_settled() FROM PUBLIC;
CREATE TRIGGER asset_jobs_settle_reservation AFTER UPDATE OF status ON asset_jobs
    FOR EACH ROW WHEN (NEW.status IN ('succeeded', 'failed', 'superseded') AND OLD.status IS DISTINCT FROM NEW.status)
    EXECUTE FUNCTION colophon_usage_job_settled();
CREATE TRIGGER asset_jobs_drop_reservation AFTER DELETE ON asset_jobs
    FOR EACH ROW EXECUTE FUNCTION colophon_usage_job_settled();

-- The limits, written by the server at boot from its environment (the last boot wins).
CREATE FUNCTION colophon_usage_set_quotas(p_entries bigint, p_assets bigint, p_storage bigint)
RETURNS void
LANGUAGE sql
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
    UPDATE public.account_quotas
    SET entries = p_entries, assets = p_assets, storage_bytes = p_storage, updated_at = now()
$$;
ALTER FUNCTION colophon_usage_set_quotas(bigint, bigint, bigint) OWNER TO colophon_accounts;
REVOKE ALL ON FUNCTION colophon_usage_set_quotas(bigint, bigint, bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_usage_set_quotas(bigint, bigint, bigint) TO colophon_api;

-- What the signed-in account holds and may hold (/api/v1/me/usage): its own row only.
CREATE FUNCTION colophon_account_usage()
RETURNS TABLE (entries bigint, assets bigint, stored_bytes bigint, pending_bytes bigint,
               max_entries bigint, max_assets bigint, max_storage_bytes bigint, exempt boolean)
LANGUAGE sql
STABLE
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
    SELECT coalesce(u.entries, 0), coalesce(u.assets, 0), coalesce(u.stored_bytes, 0),
           coalesce(u.pending_bytes, 0), q.entries, q.assets, q.storage_bytes,
           EXISTS (SELECT FROM public.administrators a WHERE a.identity_id = me.id)
    FROM (SELECT nullif(current_setting('app.identity_id', true), '')::uuid AS id) me
    CROSS JOIN public.account_quotas q
    LEFT JOIN public.account_usage u ON u.account = me.id
    WHERE me.id IS NOT NULL
$$;
ALTER FUNCTION colophon_account_usage() OWNER TO colophon_accounts;
REVOKE ALL ON FUNCTION colophon_account_usage() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_account_usage() TO colophon_api;

-- Drift alarm and repair (`colophon admin usage-reconcile [--check]`): recounts every account
-- from the rows themselves; returns the accounts whose stored totals differ, and with
-- p_repair writes the recount. Owner-only (the migration role), never the API.
CREATE FUNCTION colophon_usage_reconcile(p_repair boolean)
RETURNS TABLE (account uuid, field text, stored bigint, counted bigint)
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
#variable_conflict use_column
BEGIN
    LOCK TABLE public.account_usage IN SHARE ROW EXCLUSIVE MODE;
    CREATE TEMP TABLE IF NOT EXISTS usage_recount (account uuid PRIMARY KEY, entries bigint,
        assets bigint, stored_bytes bigint, pending_bytes bigint) ON COMMIT DROP;
    TRUNCATE usage_recount;
    INSERT INTO usage_recount
    SELECT p.owner_account,
           coalesce(sum(e.n), 0), coalesce(sum(a.n), 0), coalesce(sum(v.bytes), 0), coalesce(sum(a.pending), 0)
    FROM public.projects p
    LEFT JOIN (SELECT project_id, count(*) AS n FROM public.entries GROUP BY 1) e ON e.project_id = p.id
    LEFT JOIN (SELECT project_id, count(*) AS n,
                      coalesce(sum(size_bytes) FILTER (WHERE state = 'quarantined'), 0) AS pending
               FROM public.assets GROUP BY 1) a ON a.project_id = p.id
    LEFT JOIN (SELECT project_id, sum(size_bytes) AS bytes FROM public.asset_versions GROUP BY 1) v
           ON v.project_id = p.id
    WHERE p.owner_account IS NOT NULL
    GROUP BY 1;
    RETURN QUERY
    SELECT coalesce(u.account, r.account), f.field, f.stored, f.counted
    FROM public.account_usage u
    FULL JOIN usage_recount r ON r.account = u.account
    CROSS JOIN LATERAL (VALUES
        ('entries', coalesce(u.entries, 0), coalesce(r.entries, 0)),
        ('assets', coalesce(u.assets, 0), coalesce(r.assets, 0)),
        ('stored_bytes', coalesce(u.stored_bytes, 0), coalesce(r.stored_bytes, 0)),
        ('pending_bytes', coalesce(u.pending_bytes, 0), coalesce(r.pending_bytes, 0))) f(field, stored, counted)
    WHERE f.stored <> f.counted
    ORDER BY 1, 2;
    IF p_repair THEN
        INSERT INTO public.account_usage AS u (account, entries, assets, stored_bytes, pending_bytes)
        SELECT r.account, r.entries, r.assets, r.stored_bytes, r.pending_bytes FROM usage_recount r
        ON CONFLICT (account) DO UPDATE SET entries = excluded.entries, assets = excluded.assets,
            stored_bytes = excluded.stored_bytes, pending_bytes = excluded.pending_bytes;
        UPDATE public.account_usage u SET entries = 0, assets = 0, stored_bytes = 0, pending_bytes = 0
        WHERE NOT EXISTS (SELECT FROM usage_recount r WHERE r.account = u.account);
    END IF;
END
$$;
ALTER FUNCTION colophon_usage_reconcile(boolean) OWNER TO colophon_accounts;
REVOKE ALL ON FUNCTION colophon_usage_reconcile(boolean) FROM PUBLIC;

-- Existing installations: count what the owned projects already hold.
SELECT count(*) FROM colophon_usage_reconcile(true);
