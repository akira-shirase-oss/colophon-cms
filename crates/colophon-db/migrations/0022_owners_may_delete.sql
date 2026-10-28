-- Owners hold every permission (D-058), content.delete included. Migration 0017 seeded owners
-- without it, so no role could ever be given it: nobody grants what they don't hold. No route
-- deletes entries yet; this only makes the owner role what D-058 says it is, in every project,
-- new and existing. docs/modules/03-auth.md.

CREATE OR REPLACE FUNCTION colophon_seed_roles(project uuid) RETURNS void LANGUAGE sql AS $$
    INSERT INTO roles (project_id, key, name, description, built_in) VALUES
        (project, 'owner', 'Owner', 'Configures the project, its members and its roles.', true),
        (project, 'developer', 'Developer', 'Manages content types and website connections.', true),
        (project, 'editor', 'Editor', 'Writes and edits content.', true),
        (project, 'publisher', 'Publisher', 'Writes content, reviews it and publishes it.', true);
    INSERT INTO role_permissions (project_id, role_key, action)
    SELECT project, role_key, action FROM (VALUES
        ('owner', 'content.read'), ('owner', 'content.create'), ('owner', 'content.update'),
        ('owner', 'content.delete'), ('owner', 'schema.change'), ('owner', 'destinations.manage'),
        ('owner', 'members.manage'), ('owner', 'roles.manage'), ('owner', 'tokens.manage'),
        ('owner', 'content.review'), ('owner', 'content.publish'),
        ('developer', 'content.read'), ('developer', 'content.create'), ('developer', 'content.update'),
        ('developer', 'schema.change'), ('developer', 'destinations.manage'),
        ('editor', 'content.read'), ('editor', 'content.create'), ('editor', 'content.update'),
        ('publisher', 'content.read'), ('publisher', 'content.create'), ('publisher', 'content.update'),
        ('publisher', 'content.review'), ('publisher', 'content.publish')
    ) AS builtin (role_key, action);
$$;

-- Every existing owner role gets it. role_permissions forces row-level security on its owner too
-- (the migration login), so that is lifted for the insert and comes back on below, as 0017 does
-- for projects.
ALTER TABLE roles NO FORCE ROW LEVEL SECURITY;
ALTER TABLE role_permissions NO FORCE ROW LEVEL SECURITY;
INSERT INTO role_permissions (project_id, role_key, action)
SELECT project_id, 'owner', 'content.delete' FROM roles WHERE key = 'owner'
ON CONFLICT DO NOTHING;
ALTER TABLE roles FORCE ROW LEVEL SECURITY;
ALTER TABLE role_permissions FORCE ROW LEVEL SECURITY;
