-- D-235: per-account counting across tenants, by colophon_accounts (0087).
--
-- colophon_projects_owned(account): how many projects the account owns, whether or not it is
-- still a member of them (a project an account left still counts against it). A definer
-- function: the caller can't see other tenants' projects, and mustn't.

CREATE FUNCTION colophon_projects_owned(account uuid) RETURNS integer
    LANGUAGE sql
    STABLE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
    SELECT count(*)::integer FROM public.projects p WHERE p.owner_account = account
$$;
ALTER FUNCTION colophon_projects_owned(uuid) OWNER TO colophon_accounts;
REVOKE ALL ON FUNCTION colophon_projects_owned(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_projects_owned(uuid) TO colophon_api;
