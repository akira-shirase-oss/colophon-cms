-- Counting a sign-in attempt in one statement (D-250): the account's advisory lock, the key's row
-- lock, the account's untrusted misses and the count, in that order, as `throttle::count` ran them
-- statement by statement inside a transaction (D-154, D-162). A function keeps the order (a
-- query's CTEs don't promise one) and makes the whole step one round trip, so a sign-in waits on
-- the database a few times rather than a dozen. It runs with the caller's own privileges: the API
-- role for staff accounts, the audience role for a site's users, on the table each already writes.
--
-- Returns true when the key is paused: the attempt is refused without verifying and not counted
-- (a pause doesn't grow). Otherwise the attempt is counted and the answer is false: verify.
CREATE FUNCTION colophon_sign_in_count(
    p_realm text,
    p_email text,
    p_address text,
    p_lock_after integer,
    p_lock_for_secs double precision,
    p_account_lock_after bigint,
    p_window_secs double precision
) RETURNS boolean
    LANGUAGE plpgsql
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    key_paused boolean;
    key_trusted boolean;
    misses bigint;
BEGIN
    -- One attempt at an account at a time: the account's lock, then the key's row.
    PERFORM pg_advisory_xact_lock(22355, hashtext(p_realm || '/' || p_email));
    SELECT coalesce(t.locked_until > now(), false), t.trusted
      INTO key_paused, key_trusted
      FROM public.sign_in_throttle t
     WHERE t.realm = p_realm AND t.email = p_email AND t.address = p_address
       FOR UPDATE;
    IF FOUND AND key_paused THEN
        RETURN true;
    END IF;
    -- The account's ceiling, for addresses it has never let in: their misses summed.
    IF NOT (FOUND AND key_trusted) THEN
        SELECT coalesce(sum(t.failures), 0)::bigint INTO misses
          FROM public.sign_in_throttle t
         WHERE t.realm = p_realm AND t.email = p_email AND NOT t.trusted
           AND t.updated_at > now() - make_interval(secs => p_window_secs);
        IF misses >= p_account_lock_after THEN
            RETURN true;
        END IF;
    END IF;
    INSERT INTO public.sign_in_throttle AS t (realm, email, address, failures, locked_until)
    VALUES (p_realm, p_email, p_address, 1,
            CASE WHEN 1 >= p_lock_after THEN now() + make_interval(secs => p_lock_for_secs) END)
    ON CONFLICT (realm, email, address) DO UPDATE SET
        failures = CASE WHEN t.locked_until > now() THEN t.failures
                        WHEN t.locked_until IS NOT NULL THEN 1
                        ELSE least(t.failures + 1, 1000000) END,
        locked_until = CASE
            WHEN t.locked_until > now() THEN t.locked_until
            WHEN (CASE WHEN t.locked_until IS NOT NULL THEN 1 ELSE t.failures + 1 END) >= p_lock_after
                THEN now() + make_interval(secs => p_lock_for_secs)
        END,
        updated_at = now();
    RETURN false;
END
$$;
REVOKE ALL ON FUNCTION colophon_sign_in_count(text, text, text, integer, double precision, bigint, double precision) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_sign_in_count(text, text, text, integer, double precision, bigint, double precision)
    TO colophon_api, colophon_audience;
