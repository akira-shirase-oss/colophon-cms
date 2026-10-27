-- Milestone P3, step 3.7: the first administrator, from the Studio, with a code only the operator
-- can read. docs/modules/03-auth.md ("Owner bootstrap"), D-060 (amends D-043).
--
-- While an installation has no administrator, colophon-server issues a one-time setup code when
-- it starts and prints it to its own terminal, never to its structured log. A signed-in person
-- who types it into the Studio becomes the first administrator, exactly as
-- `colophon admin bootstrap` would have made them. Only the code's SHA-256 hash is stored here.
--
-- The application role still can't read or write these tables, `installation` or
-- `administrators`. It reaches them through the four functions below, which run as the table
-- owner and check everything themselves:
--
--   colophon_issue_setup_code    stores a new code's hash, only while nobody is an administrator;
--   colophon_setup_attempt       records an attempt and applies the rate limit;
--   colophon_live_setup_codes    the hashes of unused, unexpired codes, for a constant-time
--                                comparison in the server (the code itself never reaches
--                                Postgres, so no statement log can hold it);
--   colophon_claim_setup_code    uses one code, once, and makes its holder the administrator.
--
-- Once an administrator exists, every one of them refuses, and D-043's property holds again in
-- full: nothing on the web can make an administrator.

CREATE TABLE setup_codes (
    id uuid PRIMARY KEY,
    code_hash bytea NOT NULL UNIQUE CHECK (octet_length(code_hash) = 32),
    issued_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    used_at timestamptz,
    used_by uuid REFERENCES identities (id),
    CHECK (expires_at > issued_at AND expires_at <= issued_at + interval '24 hours'),
    CHECK ((used_at IS NULL) = (used_by IS NULL))
);

-- One row per attempt to use a code, for the rate limit. Attempts are recorded only while
-- nobody is an administrator and only within the limits, so the table stays small.
CREATE TABLE setup_attempts (
    id uuid PRIMARY KEY,
    identity_id uuid NOT NULL REFERENCES identities (id),
    attempted_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX setup_attempts_by_time ON setup_attempts (attempted_at);

-- Whether the installation is set up: bootstrapped, or with an administrator however one came
-- to be. Every function below refuses once it is.
CREATE FUNCTION colophon_installation_set_up()
    RETURNS boolean
    LANGUAGE sql
    STABLE
    SET search_path = pg_catalog, pg_temp
AS $$
    SELECT EXISTS (SELECT 1 FROM public.installation)
        OR EXISTS (SELECT 1 FROM public.administrators);
$$;

-- Stores a new code's hash, valid for `lifetime` (one minute to a day). Returns when it expires,
-- or NULL, storing nothing, when the installation already has an administrator or ten codes are
-- still live (each server start issues one; the cap bounds the table).
CREATE FUNCTION colophon_issue_setup_code(code_hash bytea, lifetime interval)
    RETURNS timestamptz
    LANGUAGE plpgsql
    VOLATILE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    expires timestamptz;
BEGIN
    IF lifetime < interval '1 minute' OR lifetime > interval '24 hours' THEN
        RAISE EXCEPTION 'a setup code lives one minute to a day' USING ERRCODE = 'check_violation';
    END IF;
    IF public.colophon_installation_set_up() THEN
        RETURN NULL;
    END IF;
    -- Codes nobody used and nobody can use any more.
    DELETE FROM public.setup_codes c WHERE c.used_at IS NULL AND c.expires_at <= now();
    IF (SELECT count(*) FROM public.setup_codes c WHERE c.used_at IS NULL) >= 10 THEN
        RETURN NULL;
    END IF;
    INSERT INTO public.setup_codes (id, code_hash, expires_at)
        VALUES (gen_random_uuid(), colophon_issue_setup_code.code_hash, now() + lifetime)
        RETURNING expires_at INTO expires;
    RETURN expires;
END
$$;

-- Records an attempt by `identity` to use a setup code, if it may make one now. `outcome` is
-- `allowed` (recorded), `limited` (not recorded; `retry_after` seconds until the oldest counted
-- attempt leaves the window) or `already_set_up` (not recorded: an administrator exists). The
-- limits: five attempts per person and twenty in all, in any fifteen minutes. Attempts queue on
-- a lock, so concurrent ones can't both take the last place.
CREATE FUNCTION colophon_setup_attempt(identity uuid)
    RETURNS TABLE (outcome text, retry_after integer)
    LANGUAGE plpgsql
    VOLATILE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    mine bigint;
    everyone bigint;
    mine_oldest timestamptz;
    everyone_oldest timestamptz;
    free_at timestamptz;
BEGIN
    IF public.colophon_installation_set_up() THEN
        RETURN QUERY SELECT 'already_set_up'::text, 0;
        RETURN;
    END IF;
    LOCK TABLE public.setup_attempts IN SHARE ROW EXCLUSIVE MODE;
    DELETE FROM public.setup_attempts a WHERE a.attempted_at <= now() - interval '15 minutes';
    SELECT count(*) FILTER (WHERE a.identity_id = colophon_setup_attempt.identity),
           min(a.attempted_at) FILTER (WHERE a.identity_id = colophon_setup_attempt.identity),
           count(*),
           min(a.attempted_at)
        INTO mine, mine_oldest, everyone, everyone_oldest
        FROM public.setup_attempts a;
    IF mine >= 5 OR everyone >= 20 THEN
        -- Room comes back when the oldest attempt counted against each exceeded limit leaves
        -- the window; greatest() skips the limit that isn't exceeded (NULL).
        free_at := greatest(
            CASE WHEN mine >= 5 THEN mine_oldest END,
            CASE WHEN everyone >= 20 THEN everyone_oldest END
        ) + interval '15 minutes';
        RETURN QUERY SELECT 'limited'::text,
            greatest(1, ceil(extract(epoch FROM free_at - now())))::integer;
        RETURN;
    END IF;
    INSERT INTO public.setup_attempts (id, identity_id) VALUES (gen_random_uuid(), identity);
    RETURN QUERY SELECT 'allowed'::text, 0;
END
$$;

-- The codes that can still be used: unused, unexpired, and only while nobody is an
-- administrator. At most ten, by the cap above.
CREATE FUNCTION colophon_live_setup_codes()
    RETURNS TABLE (id uuid, code_hash bytea)
    LANGUAGE sql
    STABLE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
    SELECT c.id, c.code_hash FROM public.setup_codes c
    WHERE c.used_at IS NULL AND c.expires_at > now()
      AND NOT public.colophon_installation_set_up()
    ORDER BY c.issued_at
    LIMIT 10;
$$;

-- Uses the code `code_id`, whose hash must be `code_hash`, to make `identity` the installation's
-- first administrator, in one transaction: `claimed`; `invalid` when the code is unknown, used,
-- expired or has another hash; `already_set_up` when someone is an administrator already (the
-- code stays unused). Claims queue on the code's row and on the installation's single row, so
-- one code makes one administrator and two codes can't make two.
CREATE FUNCTION colophon_claim_setup_code(identity uuid, code_id uuid, code_hash bytea)
    RETURNS text
    LANGUAGE plpgsql
    VOLATILE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    code public.setup_codes%ROWTYPE;
    claimed bigint;
BEGIN
    IF public.colophon_installation_set_up() THEN
        RETURN 'already_set_up';
    END IF;
    SELECT * INTO code FROM public.setup_codes c WHERE c.id = code_id FOR UPDATE;
    IF NOT FOUND OR code.code_hash <> colophon_claim_setup_code.code_hash
       OR code.used_at IS NOT NULL OR code.expires_at <= now() THEN
        -- A claim of this same code that held its row before us has just won.
        IF public.colophon_installation_set_up() THEN
            RETURN 'already_set_up';
        END IF;
        RETURN 'invalid';
    END IF;
    -- A claim that waited for another to commit finds the installation's row taken here.
    INSERT INTO public.installation (bootstrapped_by) VALUES (identity)
        ON CONFLICT (singleton) DO NOTHING;
    GET DIAGNOSTICS claimed = ROW_COUNT;
    IF claimed = 0 THEN
        RETURN 'already_set_up';
    END IF;
    UPDATE public.setup_codes c SET used_at = now(), used_by = identity WHERE c.id = code_id;
    INSERT INTO public.administrators (identity_id) VALUES (identity);
    RETURN 'claimed';
END
$$;

REVOKE ALL ON FUNCTION colophon_installation_set_up() FROM PUBLIC;
REVOKE ALL ON FUNCTION colophon_issue_setup_code(bytea, interval) FROM PUBLIC;
REVOKE ALL ON FUNCTION colophon_setup_attempt(uuid) FROM PUBLIC;
REVOKE ALL ON FUNCTION colophon_live_setup_codes() FROM PUBLIC;
REVOKE ALL ON FUNCTION colophon_claim_setup_code(uuid, uuid, bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_installation_set_up() TO colophon_api;
GRANT EXECUTE ON FUNCTION colophon_issue_setup_code(bytea, interval) TO colophon_api;
GRANT EXECUTE ON FUNCTION colophon_setup_attempt(uuid) TO colophon_api;
GRANT EXECUTE ON FUNCTION colophon_live_setup_codes() TO colophon_api;
GRANT EXECUTE ON FUNCTION colophon_claim_setup_code(uuid, uuid, bytea) TO colophon_api;
