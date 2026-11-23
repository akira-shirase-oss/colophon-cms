-- D-227 (a): a request's row-level-security context in one round trip instead of four.
--
-- On a remote database every statement is a round trip, and every authenticated project request paid four of
-- them after BEGIN to set its context: the identity, the membership (or the token) and its permissions, the
-- project, the environment. These functions run exactly those statements, in exactly that order, inside the
-- database: a plpgsql function starts each of its statements afresh, so each `current_setting` an RLS policy
-- reads sees the `set_config` before it, as when they were separate round trips. The order is the guarantee:
-- a member's project is set only once the membership is proven, so a non-member never reads in its context.
--
-- SECURITY INVOKER: they run as the caller, under the caller's policies, with no privilege of their own. The
-- search_path is pinned so no object earlier on a path can stand in for the tables they name; a SET clause
-- restores only search_path at exit, and the context settings persist for the rest of the transaction (proved
-- by tests/enter_project.rs).

-- A member: the identity alone, then the membership and its role's permissions (today's statement, verbatim),
-- and only then the project and the environment. No row: the caller isn't a member, and no project was set.
CREATE FUNCTION colophon_enter_project(p_identity uuid, p_project uuid, p_environment text)
RETURNS TABLE (role text, actions text[], subjects text[], fields text[], conditions text[], environment_id uuid)
LANGUAGE plpgsql SECURITY INVOKER SET search_path = public, pg_temp AS $$
BEGIN
  PERFORM set_config('app.identity_id', p_identity::text, true), set_config('app.project_id', '', true);
  SELECT m.role,
         coalesce(p.actions, '{}'),
         coalesce(p.subjects, '{}'),
         coalesce(p.fields, '{}'),
         coalesce(p.conditions, '{}')
    INTO role, actions, subjects, fields, conditions
    FROM memberships m
    LEFT JOIN LATERAL (
        SELECT array_agg(rp.action ORDER BY rp.action, rp.subject) AS actions,
               array_agg(rp.subject ORDER BY rp.action, rp.subject) AS subjects,
               array_agg(array_to_string(rp.fields, ',') ORDER BY rp.action, rp.subject) AS fields,
               array_agg(rp.condition ORDER BY rp.action, rp.subject) AS conditions
        FROM role_permissions rp
        WHERE rp.project_id = m.project_id AND rp.role_key = m.role
    ) p ON true
    WHERE m.project_id = p_project AND m.identity_id = p_identity;
  IF NOT FOUND THEN
    RETURN;
  END IF;
  PERFORM set_config('app.project_id', p_project::text, true);
  IF p_environment IS NOT NULL THEN
    SELECT e.id INTO environment_id FROM environments e WHERE e.project_id = p_project AND e.slug = p_environment;
  END IF;
  RETURN NEXT;
END $$;

-- A project's API token acting as its identity (D-063): the identity and the project together, then the token's
-- own check, with the share lock that makes a revocation wait for requests already under the token, then its
-- permissions and the environment (today's statements, verbatim). No row: no live token, or none here.
CREATE FUNCTION colophon_enter_project_as_token(p_identity uuid, p_project uuid, p_environment text)
RETURNS TABLE (token_id uuid, requests_per_minute bigint, actions text[], subjects text[], fields text[],
               conditions text[], environment_id uuid)
LANGUAGE plpgsql SECURITY INVOKER SET search_path = public, pg_temp AS $$
BEGIN
  PERFORM set_config('app.identity_id', p_identity::text, true), set_config('app.project_id', p_project::text, true);
  SELECT t.id, l.requests_per_minute
    INTO token_id, requests_per_minute
    FROM api_tokens t
    LEFT JOIN api_token_limits l ON l.project_id = t.project_id AND l.token_id = t.id
    WHERE t.project_id = p_project AND t.identity_id = p_identity
      AND t.revoked_at IS NULL AND t.expires_at > now()
    FOR SHARE OF t;
  IF NOT FOUND THEN
    RETURN;
  END IF;
  SELECT coalesce(array_agg(tp.action ORDER BY tp.action, tp.subject), '{}'),
         coalesce(array_agg(tp.subject ORDER BY tp.action, tp.subject), '{}'),
         coalesce(array_agg(array_to_string(tp.fields, ',') ORDER BY tp.action, tp.subject), '{}'),
         coalesce(array_agg(tp.condition ORDER BY tp.action, tp.subject), '{}')
    INTO actions, subjects, fields, conditions
    FROM api_token_permissions tp
    WHERE tp.project_id = p_project AND tp.token_id = colophon_enter_project_as_token.token_id;
  IF p_environment IS NOT NULL THEN
    SELECT e.id INTO environment_id FROM environments e WHERE e.project_id = p_project AND e.slug = p_environment;
  END IF;
  RETURN NEXT;
END $$;

REVOKE ALL ON FUNCTION colophon_enter_project(uuid, uuid, text) FROM PUBLIC;
REVOKE ALL ON FUNCTION colophon_enter_project_as_token(uuid, uuid, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_enter_project(uuid, uuid, text) TO colophon_api;
GRANT EXECUTE ON FUNCTION colophon_enter_project_as_token(uuid, uuid, text) TO colophon_api;
