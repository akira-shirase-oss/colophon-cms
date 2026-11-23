-- D-235, part 6b: an administrator disables or enables an account. Disabling is one transaction:
-- the account is marked, every session it holds and every API token it created is revoked, and
-- the staff auth log gets its row. A disabled account signs in to nothing (its password answers
-- like a wrong one) and gets no new session by any route (identity::create_session).
--
-- colophon_account_set_disabled is the only writer, owned by colophon_accounts. The acting
-- administrator is the transaction's own identity (app.identity_id, set by scope::begin), never
-- an argument. One advisory lock serialises every change, so two administrators disabling each
-- other at once leave one enabled: the second finds its actor disabled and is refused.

ALTER TABLE identities ADD COLUMN disabled_at timestamptz;

GRANT SELECT (id, issuer, disabled_at), UPDATE (disabled_at) ON identities TO colophon_accounts;
GRANT SELECT (identity_id, revoked_at), UPDATE (revoked_at) ON sessions TO colophon_accounts;
GRANT SELECT (created_by, revoked_at), UPDATE (revoked_at, revoked_by) ON api_tokens TO colophon_accounts;
CREATE POLICY api_tokens_revoked_by_accounts ON api_tokens FOR SELECT TO colophon_accounts USING (true);
CREATE POLICY api_tokens_revoke_by_accounts ON api_tokens FOR UPDATE TO colophon_accounts
    USING (true) WITH CHECK (revoked_at IS NOT NULL);
GRANT INSERT ON staff_auth_events TO colophon_accounts;
-- Revoking tokens fires their live-list hint (0068), which runs as this role here.
GRANT INSERT ON change_hints TO colophon_accounts;

-- 'done', 'unchanged' (already so), 'not_found' (no such account; an API token's identity isn't
-- one), 'not_admin' (the caller isn't an enabled administrator), 'self' (nobody disables their
-- own account), 'last_admin' (the last enabled administrator stays enabled).
CREATE FUNCTION colophon_account_set_disabled(p_target uuid, p_disable boolean, p_event uuid)
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
