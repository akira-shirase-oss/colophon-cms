-- Milestone R, step R.2: roles owners define. docs/modules/03-auth.md, 02-db.md, D-058.
--
-- Roles other than `owner` can be created, changed and deleted. A role has a version, so two
-- people editing one role can't overwrite each other unseen (If-Match, as content types do). The
-- owner role is fixed: its row and its permissions refuse every change and deletion here, so no
-- code path can leave a project whose owners can't manage it.
--
-- A role someone holds can't be deleted: memberships keep their foreign key (0017). Invitations
-- lose theirs, because accepted, revoked and expired invitations are history and mustn't pin a
-- role forever; instead, inviting holds a shared advisory lock on the role it offers, and deleting
-- a role takes it exclusively and refuses while a pending invitation offers it
-- (colophon_db::roles).

ALTER TABLE roles
    ADD COLUMN version integer NOT NULL DEFAULT 1 CHECK (version >= 1),
    ADD COLUMN updated_at timestamptz NOT NULL DEFAULT now();

ALTER TABLE invitations DROP CONSTRAINT invitations_project_id_role_fkey;
CREATE INDEX invitations_pending_role ON invitations (project_id, role)
    WHERE accepted_at IS NULL AND revoked_at IS NULL;

CREATE FUNCTION colophon_owner_role_is_fixed() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'the owner role is fixed: it always holds every permission'
        USING ERRCODE = 'insufficient_privilege';
END;
$$;

CREATE TRIGGER roles_owner_is_fixed
    BEFORE UPDATE OR DELETE ON roles
    FOR EACH ROW WHEN (OLD.key = 'owner')
    EXECUTE FUNCTION colophon_owner_role_is_fixed();
CREATE TRIGGER role_permissions_owner_is_fixed
    BEFORE UPDATE OR DELETE ON role_permissions
    FOR EACH ROW WHEN (OLD.role_key = 'owner')
    EXECUTE FUNCTION colophon_owner_role_is_fixed();

-- Writes to a project's roles queue on its owner role's row (SELECT ... FOR NO KEY UPDATE, which
-- needs UPDATE on a column), and change only these columns.
GRANT UPDATE (name, description, version, updated_at) ON roles TO colophon_api;
GRANT DELETE ON roles, role_permissions TO colophon_api;
