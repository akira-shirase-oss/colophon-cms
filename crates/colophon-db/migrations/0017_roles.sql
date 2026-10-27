-- Milestone R, step R.1: roles as data, behaviour unchanged. docs/modules/03-auth.md, 02-db.md,
-- D-058.
--
-- A project's roles each hold permissions: an action, what it applies to (one collection or `*`),
-- and, for later steps, a field list and a condition. Memberships and invitations keep their
-- `role` column, which now names one of the project's roles. Every project starts with the four
-- built-in roles of D-042, with exactly that table's powers: colophon_seed_roles is the one place
-- that says what they are, and a test holds it equal to colophon_domain::policy::Policy::builtin.

CREATE TABLE roles (
    project_id uuid NOT NULL REFERENCES projects (id),
    key text NOT NULL CHECK (key ~ '^[a-z][a-z0-9-]{0,62}$'),
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 100),
    description text NOT NULL DEFAULT '' CHECK (char_length(description) <= 500),
    built_in boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, key)
);

CREATE TABLE role_permissions (
    project_id uuid NOT NULL,
    role_key text NOT NULL,
    action text NOT NULL CHECK (action IN (
        'content.read', 'content.create', 'content.update', 'content.delete', 'content.review',
        'content.publish', 'schema.change', 'members.manage', 'roles.manage',
        'destinations.manage', 'tokens.manage')),
    -- `*`, or a collection's slug (not a foreign key: a permission may name a collection that
    -- doesn't exist yet, and keeps meaning nothing until it does).
    subject text NOT NULL DEFAULT '*'
        CHECK (subject = '*' OR subject ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'),
    fields text[] CHECK (fields IS NULL OR cardinality(fields) BETWEEN 1 AND 100),
    condition text CHECK (condition IN ('own')),
    PRIMARY KEY (project_id, role_key, action, subject),
    FOREIGN KEY (project_id, role_key) REFERENCES roles (project_id, key) ON DELETE CASCADE
);

-- The built-in roles of a project. Owners: everything; developers: content, content types and
-- destinations; editors: content; publishers: content, review and publishing.
CREATE FUNCTION colophon_seed_roles(project uuid) RETURNS void LANGUAGE sql AS $$
    INSERT INTO roles (project_id, key, name, description, built_in) VALUES
        (project, 'owner', 'Owner', 'Configures the project, its members and its roles.', true),
        (project, 'developer', 'Developer', 'Manages content types and website connections.', true),
        (project, 'editor', 'Editor', 'Writes and edits content.', true),
        (project, 'publisher', 'Publisher', 'Writes content, reviews it and publishes it.', true);
    INSERT INTO role_permissions (project_id, role_key, action)
    SELECT project, role_key, action FROM (VALUES
        ('owner', 'content.read'), ('owner', 'content.create'), ('owner', 'content.update'),
        ('owner', 'schema.change'), ('owner', 'destinations.manage'), ('owner', 'members.manage'),
        ('owner', 'roles.manage'), ('owner', 'tokens.manage'), ('owner', 'content.review'),
        ('owner', 'content.publish'),
        ('developer', 'content.read'), ('developer', 'content.create'), ('developer', 'content.update'),
        ('developer', 'schema.change'), ('developer', 'destinations.manage'),
        ('editor', 'content.read'), ('editor', 'content.create'), ('editor', 'content.update'),
        ('publisher', 'content.read'), ('publisher', 'content.create'), ('publisher', 'content.update'),
        ('publisher', 'content.review'), ('publisher', 'content.publish')
    ) AS builtin (role_key, action);
$$;

-- Every existing project gets them. The migration login isn't a superuser and `projects` forces
-- row-level security on its owner too, so that is lifted for the seeding and comes back on below
-- (as migration 0003 does). roles and role_permissions get theirs after.
ALTER TABLE projects NO FORCE ROW LEVEL SECURITY;
SELECT colophon_seed_roles(id) FROM projects;
ALTER TABLE projects FORCE ROW LEVEL SECURITY;

-- A member's or an invitation's role is one of the project's roles.
ALTER TABLE memberships DROP CONSTRAINT memberships_role_check;
ALTER TABLE memberships ADD FOREIGN KEY (project_id, role) REFERENCES roles (project_id, key);
ALTER TABLE invitations DROP CONSTRAINT invitations_role_check;
ALTER TABLE invitations ADD FOREIGN KEY (project_id, role) REFERENCES roles (project_id, key);

ALTER TABLE roles ENABLE ROW LEVEL SECURITY;
ALTER TABLE roles FORCE ROW LEVEL SECURITY;
ALTER TABLE role_permissions ENABLE ROW LEVEL SECURITY;
ALTER TABLE role_permissions FORCE ROW LEVEL SECURITY;

-- Inside a project's context; and readable by its members before that context is set, because
-- the membership lookup reads the caller's permissions in the same statement (scope.rs).
CREATE POLICY roles_in_project ON roles FOR ALL TO colophon_api
    USING (
        project_id = nullif(current_setting('app.project_id', true), '')::uuid
        OR EXISTS (
            SELECT 1 FROM memberships m
            WHERE m.project_id = roles.project_id
              AND m.identity_id = nullif(current_setting('app.identity_id', true), '')::uuid
        )
    )
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
CREATE POLICY role_permissions_in_project ON role_permissions FOR ALL TO colophon_api
    USING (
        project_id = nullif(current_setting('app.project_id', true), '')::uuid
        OR EXISTS (
            SELECT 1 FROM memberships m
            WHERE m.project_id = role_permissions.project_id
              AND m.identity_id = nullif(current_setting('app.identity_id', true), '')::uuid
        )
    )
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);

-- R.1 only reads roles and seeds new projects'; changing them comes with R.2.
GRANT SELECT, INSERT ON roles, role_permissions TO colophon_api;
GRANT EXECUTE ON FUNCTION colophon_seed_roles(uuid) TO colophon_api;
