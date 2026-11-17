-- D-162: a per-account ceiling on sign-in guessing that the owner's own addresses bypass.
--
-- D-154 paused guessing per (account, client address), so a guesser pausing themselves could
-- never lock the owner out — but nothing bounded the account as a whole: rotating through
-- IPv6 /64s, each good for eight guesses, guessed without end. Now misses from addresses that
-- have never signed in to the account are summed per account: past fifty in an hour, every
-- such address is paused for it, answered exactly like a wrong password. An address the owner
-- already signed in from is `trusted` and keeps working, so the ceiling bounds an attack
-- without becoming a lockout of the person it protects. A key's trust is the record of a
-- success: it survives 90 days unused, an untrusted key one (the sweep below).

ALTER TABLE sign_in_throttle ADD COLUMN trusted boolean NOT NULL DEFAULT false;
CREATE INDEX sign_in_throttle_by_account ON sign_in_throttle (realm, email, updated_at)
    WHERE NOT trusted;

-- The sweep keeps trusted keys 90 days. Its function belongs to colophon_retention (0057), and
-- only an owner may replace a function — a migrator that isn't a superuser is not one — so it
-- is dropped as its owner and created again exactly as 0057 created it.
SET LOCAL ROLE colophon_retention;
DROP FUNCTION colophon_sweep_sign_in_throttle(timestamptz, bigint);
RESET ROLE;

CREATE FUNCTION colophon_sweep_sign_in_throttle(cutoff timestamptz, batch bigint)
    RETURNS bigint LANGUAGE plpgsql VOLATILE SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE removed bigint;
BEGIN
    WITH doomed AS (
        SELECT realm, email, address FROM public.sign_in_throttle
        WHERE (NOT trusted AND updated_at < cutoff
               AND (locked_until IS NULL OR locked_until < cutoff))
           OR (trusted AND updated_at < cutoff - interval '89 days')
        ORDER BY updated_at LIMIT batch)
    DELETE FROM public.sign_in_throttle t USING doomed d
        WHERE t.realm = d.realm AND t.email = d.email AND t.address = d.address;
    GET DIAGNOSTICS removed = ROW_COUNT;
    RETURN removed;
END $$;

ALTER FUNCTION colophon_sweep_sign_in_throttle(timestamptz, bigint) OWNER TO colophon_retention;
REVOKE ALL ON FUNCTION colophon_sweep_sign_in_throttle(timestamptz, bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_sweep_sign_in_throttle(timestamptz, bigint) TO colophon_worker;
