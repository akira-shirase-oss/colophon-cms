-- D-228: a project API token does what both it and its creator's role, as it is now, allow.
--
-- A token held its permissions for as long as it lived, whatever happened to the person who made it: an owner
-- removed from the project still got in through a token they had made. Now entering a project as a token also
-- needs its creator to be a member there still (no row otherwise, like a revoked token), and it reads that
-- member's role's permissions beside the token's, so the server intersects the two (Policy::intersect). A
-- demotion narrows every token its holder made at their next request; nothing is cached, so nothing has to be
-- told. Removing a member also revokes their tokens, with an audit row each (members::remove); this check is what
-- holds however else a membership ends (a restore's ledger replay).
--
-- Otherwise 0079's function, statement for statement: the settings first, the token with its share lock, its
-- permissions, the environment. The membership read is one more statement in the same round trip.

DROP FUNCTION colophon_enter_project_as_token(uuid, uuid, text);

CREATE FUNCTION colophon_enter_project_as_token(p_identity uuid, p_project uuid, p_environment text)
RETURNS TABLE (token_id uuid, requests_per_minute bigint, actions text[], subjects text[], fields text[],
               conditions text[], role_actions text[], role_subjects text[], role_fields text[],
               role_conditions text[], environment_id uuid)
LANGUAGE plpgsql SECURITY INVOKER SET search_path = public, pg_temp AS $$
DECLARE
  creator uuid;
BEGIN
  PERFORM set_config('app.identity_id', p_identity::text, true), set_config('app.project_id', p_project::text, true);
  SELECT t.id, l.requests_per_minute, t.created_by
    INTO token_id, requests_per_minute, creator
    FROM api_tokens t
    LEFT JOIN api_token_limits l ON l.project_id = t.project_id AND l.token_id = t.id
    WHERE t.project_id = p_project AND t.identity_id = p_identity
      AND t.revoked_at IS NULL AND t.expires_at > now()
    FOR SHARE OF t;
  IF NOT FOUND THEN
    RETURN;
  END IF;
  -- The creator's role as it is now; no membership, no entry.
  SELECT coalesce(p.actions, '{}'), coalesce(p.subjects, '{}'), coalesce(p.fields, '{}'),
         coalesce(p.conditions, '{}')
    INTO role_actions, role_subjects, role_fields, role_conditions
    FROM memberships m
    LEFT JOIN LATERAL (
        SELECT array_agg(rp.action ORDER BY rp.action, rp.subject) AS actions,
               array_agg(rp.subject ORDER BY rp.action, rp.subject) AS subjects,
               array_agg(array_to_string(rp.fields, ',') ORDER BY rp.action, rp.subject) AS fields,
               array_agg(rp.condition ORDER BY rp.action, rp.subject) AS conditions
        FROM role_permissions rp
        WHERE rp.project_id = m.project_id AND rp.role_key = m.role
    ) p ON true
    WHERE m.project_id = p_project AND m.identity_id = creator;
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

REVOKE ALL ON FUNCTION colophon_enter_project_as_token(uuid, uuid, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_enter_project_as_token(uuid, uuid, text) TO colophon_api;
