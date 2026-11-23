-- D-246: OAuth 2.1 sign-in for the MCP endpoint, so an agent connects by its URL alone: the client
-- registers itself (RFC 7591), a person signs in to the Studio and approves it for one of their
-- projects, and the client exchanges a single-use code (PKCE S256) for an access token and a
-- rotating refresh token. Like sessions and access tokens, every secret is stored only as its
-- SHA-256. Tables are the server's own (colophon_api), read and written by id or hash; no row is
-- ever served to anyone but the identity it belongs to.

-- A registered client. Public clients only (no secret); its name is a label it chose for itself,
-- shown as unverified. Bounded: registration is rate-limited per address, and the API refuses a
-- registration over the table's cap.
CREATE TABLE oauth_clients (
    id uuid PRIMARY KEY,
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 100),
    -- Exact redirect URIs, compared byte for byte (https, or http on a loopback host: RFC 8252).
    redirect_uris text[] NOT NULL CHECK (cardinality(redirect_uris) BETWEEN 1 AND 5),
    created_at timestamptz NOT NULL DEFAULT now(),
    last_used_at timestamptz
);
CREATE INDEX oauth_clients_by_age ON oauth_clients (created_at);

-- An authorization in progress: what /oauth/authorize checked, waiting for the person's answer on
-- the consent page, which reaches it by this id alone. Consumed by approving or denying it.
CREATE TABLE oauth_requests (
    id uuid PRIMARY KEY,
    client_id uuid NOT NULL REFERENCES oauth_clients (id) ON DELETE CASCADE,
    redirect_uri text NOT NULL,
    state text CHECK (char_length(state) <= 500),
    code_challenge text NOT NULL CHECK (code_challenge ~ '^[A-Za-z0-9_-]{43}$'),
    identity_id uuid NOT NULL REFERENCES identities (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL CHECK (expires_at > created_at)
);
CREATE INDEX oauth_requests_by_expiry ON oauth_requests (expires_at);

-- A person's approval of a client for one project: what its tokens act under. Revoked, never
-- deleted, so "connected apps" and the audit trail keep it.
CREATE TABLE oauth_grants (
    id uuid PRIMARY KEY,
    client_id uuid NOT NULL REFERENCES oauth_clients (id),
    identity_id uuid NOT NULL REFERENCES identities (id),
    -- A deleted project takes its grants (and so their codes and tokens) with it (D-167's ledger).
    project_id uuid NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now(),
    last_used_at timestamptz,
    revoked_at timestamptz
);
CREATE INDEX oauth_grants_by_identity ON oauth_grants (identity_id) WHERE revoked_at IS NULL;

-- A single-use authorization code, bound to its grant, client, redirect URI and PKCE challenge.
CREATE TABLE oauth_codes (
    code_hash bytea PRIMARY KEY CHECK (length(code_hash) = 32),
    grant_id uuid NOT NULL REFERENCES oauth_grants (id) ON DELETE CASCADE,
    redirect_uri text NOT NULL,
    code_challenge text NOT NULL CHECK (code_challenge ~ '^[A-Za-z0-9_-]{43}$'),
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL CHECK (expires_at > created_at),
    used_at timestamptz
);
CREATE INDEX oauth_codes_by_expiry ON oauth_codes (expires_at);

-- Access tokens (an hour) and refresh tokens (thirty days, single use: each refresh replaces it,
-- and one presented again after its use revokes the whole grant).
CREATE TABLE oauth_tokens (
    token_hash bytea PRIMARY KEY CHECK (length(token_hash) = 32),
    grant_id uuid NOT NULL REFERENCES oauth_grants (id) ON DELETE CASCADE,
    kind text NOT NULL CHECK (kind IN ('access', 'refresh')),
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL CHECK (expires_at > created_at),
    used_at timestamptz
);
CREATE INDEX oauth_tokens_by_grant ON oauth_tokens (grant_id);
CREATE INDEX oauth_tokens_by_expiry ON oauth_tokens (expires_at);

GRANT SELECT, INSERT, DELETE, UPDATE (last_used_at) ON oauth_clients TO colophon_api;
GRANT SELECT, INSERT, DELETE ON oauth_requests TO colophon_api;
GRANT SELECT, INSERT, UPDATE (last_used_at, revoked_at) ON oauth_grants TO colophon_api;
GRANT SELECT, INSERT, DELETE, UPDATE (used_at) ON oauth_codes TO colophon_api;
GRANT SELECT, INSERT, DELETE, UPDATE (used_at) ON oauth_tokens TO colophon_api;
-- Disabling an account (0096) revokes its grants in the same transaction.
GRANT SELECT (identity_id, revoked_at), UPDATE (revoked_at) ON oauth_grants TO colophon_accounts;

-- 0096's function, the same but for one line: disabling an account revokes its OAuth grants too,
-- in the same transaction as its sessions and the API tokens it made.
CREATE OR REPLACE FUNCTION colophon_account_set_disabled(p_target uuid, p_disable boolean, p_event uuid)
RETURNS text
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    actor uuid := nullif(current_setting('app.identity_id', true), '')::uuid;
BEGIN
    PERFORM pg_advisory_xact_lock(hashtext('colophon:account-disable'));
    IF actor IS NULL OR NOT EXISTS (
        SELECT FROM public.administrators a JOIN public.identities i ON i.id = a.identity_id
        WHERE a.identity_id = actor AND i.disabled_at IS NULL
    ) THEN
        RETURN 'not_admin';
    END IF;
    IF p_target = actor THEN
        RETURN 'self';
    END IF;
    PERFORM FROM public.identities
    WHERE id = p_target AND issuer <> 'urn:colophon:api-token'
    FOR UPDATE;
    IF NOT FOUND THEN
        RETURN 'not_found';
    END IF;
    IF p_disable THEN
        IF EXISTS (SELECT FROM public.administrators WHERE identity_id = p_target)
           AND NOT EXISTS (
               SELECT FROM public.administrators a JOIN public.identities i ON i.id = a.identity_id
               WHERE a.identity_id <> p_target AND i.disabled_at IS NULL)
        THEN
            RETURN 'last_admin';
        END IF;
        UPDATE public.identities SET disabled_at = now() WHERE id = p_target AND disabled_at IS NULL;
        IF NOT FOUND THEN
            RETURN 'unchanged';
        END IF;
        UPDATE public.sessions SET revoked_at = now() WHERE identity_id = p_target AND revoked_at IS NULL;
        UPDATE public.api_tokens SET revoked_at = now(), revoked_by = actor
        WHERE created_by = p_target AND revoked_at IS NULL;
        UPDATE public.oauth_grants SET revoked_at = now() WHERE identity_id = p_target AND revoked_at IS NULL;
        INSERT INTO public.staff_auth_events (id, event, identity_id, actor_id)
        VALUES (p_event, 'account_disabled', p_target, actor);
    ELSE
        UPDATE public.identities SET disabled_at = NULL WHERE id = p_target AND disabled_at IS NOT NULL;
        IF NOT FOUND THEN
            RETURN 'unchanged';
        END IF;
        INSERT INTO public.staff_auth_events (id, event, identity_id, actor_id)
        VALUES (p_event, 'account_enabled', p_target, actor);
    END IF;
    RETURN 'done';
END
$$;
ALTER FUNCTION colophon_account_set_disabled(uuid, boolean, uuid) OWNER TO colophon_accounts;
REVOKE ALL ON FUNCTION colophon_account_set_disabled(uuid, boolean, uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_account_set_disabled(uuid, boolean, uuid) TO colophon_api;
