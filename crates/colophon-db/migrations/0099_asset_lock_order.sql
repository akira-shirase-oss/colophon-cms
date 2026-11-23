-- One lock order for an asset row and its owner's counters (D-249): the asset row first, then
-- the account's usage row.
--
-- The worker already takes them in that order: landing a verdict updates the asset (a key update,
-- `public_key` being unique) and its statement trigger then moves the account's pending bytes
-- (`assets_usage_update` → `account_usage`); deleting and expiring lock the asset FOR UPDATE
-- before anything else. A save that names an image took them the other way round: creating the
-- entry moved the account's entry count (`entries_usage_insert`), and only then did the reference
-- insert take the asset's FOR KEY SHARE (migration 0039's trigger). A save and a landing of the
-- same image could each hold the row the other waited for: a deadlock, the save answering 503 or
-- the landing waiting out its lease.
--
-- A save now takes FOR KEY SHARE on every asset it names, in id order, before it writes anything.
-- The API role may not lock `assets` itself (it may not update it, migration 0035), so, as 0039's
-- trigger does, this runs as the table's worker role. It reads the project from the transaction's
-- row-security context rather than an argument, so a caller can only ever lock rows of the
-- project it has already entered, and it returns nothing.
CREATE FUNCTION colophon_lock_assets(p_environment uuid, p_ids uuid[]) RETURNS void
    LANGUAGE plpgsql
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    PERFORM 1 FROM public.assets a
    WHERE a.project_id = current_setting('app.project_id')::uuid
      AND a.environment_id = p_environment
      AND a.id = ANY (p_ids)
    ORDER BY a.id
    FOR KEY SHARE;
END
$$;
ALTER FUNCTION colophon_lock_assets(uuid, uuid[]) OWNER TO colophon_worker;
REVOKE ALL ON FUNCTION colophon_lock_assets(uuid, uuid[]) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_lock_assets(uuid, uuid[]) TO colophon_api;
