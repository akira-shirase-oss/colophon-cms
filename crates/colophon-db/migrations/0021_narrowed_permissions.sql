-- Milestone R, step R.3: permissions narrowed to some fields, or to a member's own entries.
-- docs/modules/03-auth.md, D-058.
--
-- A role may hold one action on a collection twice: for every entry, and for the entries its
-- members created (with different fields, say: titles of any article, every field of their own).
-- So a permission is one row per action, subject and condition, and its fields are the union the
-- domain merges (colophon_domain::policy::Policy::new). Entries record who created them already.

ALTER TABLE role_permissions DROP CONSTRAINT role_permissions_pkey;
CREATE UNIQUE INDEX role_permissions_key
    ON role_permissions (project_id, role_key, action, subject, (coalesce(condition, '')));

-- Field ids, as content types write them.
ALTER TABLE role_permissions ADD CONSTRAINT role_permissions_field_ids
    CHECK (fields IS NULL OR array_to_string(fields, ',') ~ '^[a-z][a-z0-9_]{0,63}(,[a-z][a-z0-9_]{0,63})*$');
-- Field lists only where fields mean something; own only where an owner does.
ALTER TABLE role_permissions ADD CONSTRAINT role_permissions_fields_apply
    CHECK (fields IS NULL OR action IN ('content.read', 'content.create', 'content.update'));
ALTER TABLE role_permissions ADD CONSTRAINT role_permissions_own_applies
    CHECK (condition IS NULL OR action IN ('content.read', 'content.update', 'content.delete'));
