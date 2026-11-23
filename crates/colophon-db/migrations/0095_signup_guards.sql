-- D-235: the guards an open sign-up needs. The staff auth log, and a sign-up cap per client
-- address.
--
-- staff_auth_events: who signed up, signed in, failed to, signed out or had a session rotated
-- on the staff side (the audience's own events go to audit_events, 0054). Append-only like
-- audit_events: the application role may only INSERT, a trigger refuses UPDATE, DELETE and
-- TRUNCATE from anyone but the retention sweep, which keeps it for the audit age (90 days)
-- through colophon_staff_auth_sweep, owned by colophon_retention. It holds no password, token
-- or raw email: a failure for an address with no account is recorded as an HMAC-SHA256 under
-- the installation's key (COLOPHON_AUTH_EVENT_KEY), computed by the server, or not at all; a
-- known account is recorded by its identity. The client address is the throttle's key (an IPv6
-- address by its /64).
--
-- sign_in_log_budget: how many failed sign-ins from one client address the log records in an
-- hour (D-235). Failed sign-ins aren't capped like sign-ups (D-154 pauses guessing per account and
-- address instead), so without this a spray of many emails from many addresses would grow an
-- append-only table at the attacker's rate for 90 days. The server records a miss only while
-- colophon_sign_in_log_admit's count is within its budget, one more line marked log_capped when
-- it runs out, and nothing more from that address until the hour ends.
--
-- signup_throttle: sign-ups one client address (the same key) may attempt in an hour,
-- COLOPHON_SIGNUPS_PER_ADDRESS_HOUR. Every attempt counts, refused ones included (a flood that
-- fails the bot check is not free), counted before the bot check and before Argon2, through
-- colophon_signup_attempt (owned by colophon_accounts) in its own statement. A window older than
-- an hour starts over; the retention sweep removes rows idle that long.

CREATE TABLE staff_auth_events (
    id uuid PRIMARY KEY,
    at timestamptz NOT NULL DEFAULT now(),
    event text NOT NULL CHECK (event IN (
        'signed_up', 'sign_up_refused', 'signed_in', 'sign_in_failed', 'signed_out',
        'session_rotated', 'account_disabled', 'account_enabled')),
    -- The account the event is about, when there is one.
    identity_id uuid REFERENCES identities (id),
    -- Who acted, when it isn't the account itself (an administrator disabling one).
    actor_id uuid REFERENCES identities (id),
    -- HMAC-SHA256 of the normalized email under the installation's key, for an event with no
    -- account to name (a failed sign-in for an unknown address, a refused sign-up).
    email_digest bytea CHECK (email_digest IS NULL OR length(email_digest) = 32),
    address text CHECK (char_length(address) BETWEEN 1 AND 64),
    -- A short reason: pre_setup, invitation, rate_limited, bot_check_failed, registration_closed...
    detail text CHECK (detail IS NULL OR detail ~ '^[a-z_]{1,48}$')
);
CREATE INDEX staff_auth_events_by_age ON staff_auth_events (at);
CREATE INDEX staff_auth_events_by_identity ON staff_auth_events (identity_id, at) WHERE identity_id IS NOT NULL;

GRANT INSERT ON staff_auth_events TO colophon_api;
GRANT SELECT, DELETE ON staff_auth_events TO colophon_retention;

CREATE FUNCTION colophon_staff_auth_append_only() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF tg_op = 'DELETE' AND current_setting('colophon.staff_auth_sweep', true) = 'on' THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION 'staff_auth_events is append-only' USING ERRCODE = 'insufficient_privilege';
END
$$;
CREATE TRIGGER staff_auth_events_append_only BEFORE UPDATE OR DELETE ON staff_auth_events
    FOR EACH ROW EXECUTE FUNCTION colophon_staff_auth_append_only();
CREATE TRIGGER staff_auth_events_no_truncate BEFORE TRUNCATE ON staff_auth_events
    FOR EACH STATEMENT EXECUTE FUNCTION colophon_staff_auth_append_only();

-- The one delete the staff auth log allows: oldest first, at most `batch`, older than `cutoff`.
CREATE FUNCTION colophon_staff_auth_sweep(cutoff timestamptz, batch bigint)
    RETURNS bigint
    LANGUAGE plpgsql
    VOLATILE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    removed bigint;
BEGIN
    PERFORM set_config('colophon.staff_auth_sweep', 'on', true);
    WITH doomed AS (
        SELECT id FROM public.staff_auth_events WHERE at < cutoff ORDER BY at LIMIT batch)
    DELETE FROM public.staff_auth_events e USING doomed d WHERE e.id = d.id;
    GET DIAGNOSTICS removed = ROW_COUNT;
    PERFORM set_config('colophon.staff_auth_sweep', '', true);
    RETURN removed;
END
$$;
ALTER FUNCTION colophon_staff_auth_sweep(timestamptz, bigint) OWNER TO colophon_retention;
REVOKE ALL ON FUNCTION colophon_staff_auth_sweep(timestamptz, bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_staff_auth_sweep(timestamptz, bigint) TO colophon_worker;

CREATE TABLE signup_throttle (
    address text PRIMARY KEY CHECK (char_length(address) BETWEEN 1 AND 64),
    window_started timestamptz NOT NULL,
    attempts integer NOT NULL CHECK (attempts > 0)
);
CREATE INDEX signup_throttle_by_age ON signup_throttle (window_started);
ALTER TABLE signup_throttle ENABLE ROW LEVEL SECURITY;
ALTER TABLE signup_throttle FORCE ROW LEVEL SECURITY;
CREATE POLICY signup_throttle_by_accounts ON signup_throttle TO colophon_accounts
    USING (true) WITH CHECK (true);
CREATE POLICY signup_throttle_for_retention ON signup_throttle TO colophon_retention
    USING (true) WITH CHECK (true);
GRANT SELECT, INSERT, UPDATE ON signup_throttle TO colophon_accounts;
GRANT SELECT, DELETE ON signup_throttle TO colophon_retention;

-- Counts one sign-up attempt from `p_address` and says whether it is within `p_limit` an hour;
-- when it isn't, how many seconds until the window ends. One statement: two attempts at once
-- serialize on the row, so the count never misses one.
CREATE FUNCTION colophon_signup_attempt(p_address text, p_limit integer,
                                        OUT admitted boolean, OUT retry_after integer)
    LANGUAGE plpgsql
    VOLATILE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    n integer;
    started timestamptz;
BEGIN
    INSERT INTO public.signup_throttle AS t (address, window_started, attempts)
    VALUES (p_address, now(), 1)
    ON CONFLICT (address) DO UPDATE SET
        window_started = CASE WHEN t.window_started <= now() - interval '1 hour'
                              THEN now() ELSE t.window_started END,
        attempts = CASE WHEN t.window_started <= now() - interval '1 hour'
                        THEN 1 ELSE least(t.attempts + 1, 1000000000) END
    RETURNING t.attempts, t.window_started INTO n, started;
    admitted := n <= p_limit;
    retry_after := CASE WHEN admitted THEN 0
                        ELSE greatest(1, ceil(extract(epoch FROM started + interval '1 hour' - now())))::integer
                   END;
END
$$;
ALTER FUNCTION colophon_signup_attempt(text, integer) OWNER TO colophon_accounts;
REVOKE ALL ON FUNCTION colophon_signup_attempt(text, integer) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_signup_attempt(text, integer) TO colophon_api;

CREATE FUNCTION colophon_sweep_signup_throttle(cutoff timestamptz, batch bigint)
    RETURNS bigint LANGUAGE plpgsql VOLATILE SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE removed bigint;
BEGIN
    WITH doomed AS (
        SELECT address FROM public.signup_throttle
        WHERE window_started < cutoff ORDER BY window_started LIMIT batch)
    DELETE FROM public.signup_throttle t USING doomed d WHERE t.address = d.address;
    GET DIAGNOSTICS removed = ROW_COUNT;
    RETURN removed;
END $$;
ALTER FUNCTION colophon_sweep_signup_throttle(timestamptz, bigint) OWNER TO colophon_retention;
REVOKE ALL ON FUNCTION colophon_sweep_signup_throttle(timestamptz, bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_sweep_signup_throttle(timestamptz, bigint) TO colophon_worker;

CREATE TABLE sign_in_log_budget (
    address text PRIMARY KEY CHECK (char_length(address) BETWEEN 1 AND 64),
    window_started timestamptz NOT NULL,
    misses integer NOT NULL CHECK (misses > 0)
);
CREATE INDEX sign_in_log_budget_by_age ON sign_in_log_budget (window_started);
ALTER TABLE sign_in_log_budget ENABLE ROW LEVEL SECURITY;
ALTER TABLE sign_in_log_budget FORCE ROW LEVEL SECURITY;
CREATE POLICY sign_in_log_budget_by_accounts ON sign_in_log_budget TO colophon_accounts
    USING (true) WITH CHECK (true);
CREATE POLICY sign_in_log_budget_for_retention ON sign_in_log_budget TO colophon_retention
    USING (true) WITH CHECK (true);
GRANT SELECT, INSERT, UPDATE ON sign_in_log_budget TO colophon_accounts;
GRANT SELECT, DELETE ON sign_in_log_budget TO colophon_retention;

-- Counts one failed sign-in from `p_address` and answers its number within the current hour.
CREATE FUNCTION colophon_sign_in_log_admit(p_address text) RETURNS integer
    LANGUAGE plpgsql
    VOLATILE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    n integer;
BEGIN
    INSERT INTO public.sign_in_log_budget AS b (address, window_started, misses)
    VALUES (p_address, now(), 1)
    ON CONFLICT (address) DO UPDATE SET
        window_started = CASE WHEN b.window_started <= now() - interval '1 hour'
                              THEN now() ELSE b.window_started END,
        misses = CASE WHEN b.window_started <= now() - interval '1 hour'
                      THEN 1 ELSE least(b.misses + 1, 1000000000) END
    RETURNING b.misses INTO n;
    RETURN n;
END
$$;
ALTER FUNCTION colophon_sign_in_log_admit(text) OWNER TO colophon_accounts;
REVOKE ALL ON FUNCTION colophon_sign_in_log_admit(text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_sign_in_log_admit(text) TO colophon_api;

CREATE FUNCTION colophon_sweep_sign_in_log_budget(cutoff timestamptz, batch bigint)
    RETURNS bigint LANGUAGE plpgsql VOLATILE SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE removed bigint;
BEGIN
    WITH doomed AS (
        SELECT address FROM public.sign_in_log_budget
        WHERE window_started < cutoff ORDER BY window_started LIMIT batch)
    DELETE FROM public.sign_in_log_budget t USING doomed d WHERE t.address = d.address;
    GET DIAGNOSTICS removed = ROW_COUNT;
    RETURN removed;
END $$;
ALTER FUNCTION colophon_sweep_sign_in_log_budget(timestamptz, bigint) OWNER TO colophon_retention;
REVOKE ALL ON FUNCTION colophon_sweep_sign_in_log_budget(timestamptz, bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_sweep_sign_in_log_budget(timestamptz, bigint) TO colophon_worker;

-- The session cookies' rename to __Host-colophon_session / __Host-colophon_csrf (D-235). For one
-- release the server still reads the old name, but only for a session made before this
-- migration ran: an old-name cookie can be planted from a sibling subdomain (cookie tossing), so
-- a session made after the rename is never accepted under it. The first use of an accepted one
-- swaps it for a new session under the new name.
CREATE TABLE session_cookie_rename (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    at timestamptz NOT NULL DEFAULT now()
);
INSERT INTO session_cookie_rename DEFAULT VALUES;
GRANT SELECT ON session_cookie_rename TO colophon_api;
