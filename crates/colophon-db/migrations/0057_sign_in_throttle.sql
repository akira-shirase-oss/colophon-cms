-- D-154: sign-in guessing is paused per (account, client address), not per account.
--
-- Until now eight consecutive misses paused the *account* for fifteen minutes, and a miss
-- after the pause re-paused it at once — so anyone who knew an address could keep its owner
-- locked out forever with one guess every quarter hour, and the distinct `409
-- too_many_attempts` told them which addresses had accounts at all. The counter now lives
-- here, keyed by the realm (`staff`, or `audience:<project>`), the normalized email and the
-- client address the request came from: a guesser pauses only their own address, the
-- owner's address keeps working, and since the key doesn't depend on whether the account
-- exists, a pause says nothing about it either — the API answers a paused attempt with the
-- very refusal a wrong password gets.
--
-- No row-level security, like `sign_in_attempts`: these rows are read before any project or
-- identity context exists, and hold no secret. They are bounded by the sweep: a key untouched
-- for a day starts over (`colophon_sweep_sign_in_throttle`, run by the worker as
-- `colophon_retention`). The accounts' own `failed_sign_ins` / `locked_until` columns
-- (migrations 0032, 0052) are no longer written; they stay for now so no migration drops data.

CREATE TABLE sign_in_throttle (
    realm text NOT NULL CHECK (char_length(realm) BETWEEN 1 AND 64),
    email text NOT NULL CHECK (char_length(email) BETWEEN 1 AND 254),
    address text NOT NULL CHECK (char_length(address) BETWEEN 1 AND 64),
    failures integer NOT NULL DEFAULT 0 CHECK (failures >= 0),
    locked_until timestamptz,
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (realm, email, address)
);
CREATE INDEX sign_in_throttle_by_age ON sign_in_throttle (updated_at);

GRANT SELECT, INSERT, UPDATE, DELETE ON sign_in_throttle TO colophon_api;
GRANT SELECT, DELETE ON sign_in_throttle TO colophon_retention;

CREATE FUNCTION colophon_sweep_sign_in_throttle(cutoff timestamptz, batch bigint)
    RETURNS bigint LANGUAGE plpgsql VOLATILE SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE removed bigint;
BEGIN
    WITH doomed AS (
        SELECT realm, email, address FROM public.sign_in_throttle
        WHERE updated_at < cutoff AND (locked_until IS NULL OR locked_until < cutoff)
        ORDER BY updated_at LIMIT batch)
    DELETE FROM public.sign_in_throttle t USING doomed d
        WHERE t.realm = d.realm AND t.email = d.email AND t.address = d.address;
    GET DIAGNOSTICS removed = ROW_COUNT;
    RETURN removed;
END $$;

ALTER FUNCTION colophon_sweep_sign_in_throttle(timestamptz, bigint) OWNER TO colophon_retention;
REVOKE ALL ON FUNCTION colophon_sweep_sign_in_throttle(timestamptz, bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_sweep_sign_in_throttle(timestamptz, bigint) TO colophon_worker;
