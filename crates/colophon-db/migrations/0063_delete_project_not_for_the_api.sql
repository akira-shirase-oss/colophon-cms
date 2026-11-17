-- D-167: colophon_delete_project is not the application's to call.
--
-- 0048 granted EXECUTE to colophon_api, and the function's only check is that the caller's
-- `app.identity_id` and `app.project_id` settings are set and name the project — settings any
-- session can set, which D-155 already ruled are not credentials. No route calls it: the
-- ledger replay runs it as colophon_deleter, and an operator deletes a project from the
-- owner's login. So the application loses EXECUTE until a deletion route exists with an
-- authorization check of its own.
--
-- Revoked both as the migrator and as the owner (SET LOCAL ROLE, as 0061 needs): a REVOKE
-- removes only the grants its own role made, and 0048 and 0061 granted it after handing the
-- function to colophon_deleter.

REVOKE EXECUTE ON FUNCTION colophon_delete_project(uuid) FROM colophon_api;
SET LOCAL ROLE colophon_deleter;
REVOKE EXECUTE ON FUNCTION colophon_delete_project(uuid) FROM colophon_api;
RESET ROLE;
