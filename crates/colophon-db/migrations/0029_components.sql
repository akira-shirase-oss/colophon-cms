-- Milestone M, step M.4: shared components. docs/modules/01-domain.md, 02-db.md, D-073.
--
-- A component is a named group of fields many content types use, versioned like a content type:
-- `components` is its identity and `component_versions` its insert-only definitions. A content
-- type holds a copy of the component's fields, marked `"component":"<slug>"` in its canonical
-- definition, so a definition still says everything about its content and the hashes that
-- publish requests pin stay true. Changing a component writes a new version of every content
-- type that uses it, in the same transaction, each checked against its stored content
-- (components::lock_for_change, then the content types' own locks).

CREATE TABLE components (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    slug text NOT NULL CHECK (slug ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, environment_id, slug),
    FOREIGN KEY (project_id, environment_id) REFERENCES environments (project_id, id)
);

CREATE TABLE component_versions (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    component text NOT NULL,
    version integer NOT NULL CHECK (version >= 1),
    previous_version integer,
    -- The canonical definition (Component::canonical_json), kept as text with its SHA-256.
    definition text NOT NULL CHECK (octet_length(definition) <= 65536),
    definition_hash bytea NOT NULL CHECK (length(definition_hash) = 32),
    created_by uuid NOT NULL REFERENCES identities (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, environment_id, component, version),
    FOREIGN KEY (project_id, environment_id, component)
        REFERENCES components (project_id, environment_id, slug),
    FOREIGN KEY (project_id, environment_id, component, previous_version)
        REFERENCES component_versions (project_id, environment_id, component, version),
    CHECK ((version = 1 AND previous_version IS NULL) OR previous_version = version - 1)
);

ALTER TABLE components ENABLE ROW LEVEL SECURITY;
ALTER TABLE components FORCE ROW LEVEL SECURITY;
ALTER TABLE component_versions ENABLE ROW LEVEL SECURITY;
ALTER TABLE component_versions FORCE ROW LEVEL SECURITY;
CREATE POLICY components_in_project ON components FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
CREATE POLICY component_versions_in_project ON component_versions FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);

GRANT SELECT, INSERT ON components TO colophon_api;
GRANT SELECT, INSERT ON component_versions TO colophon_api;

ALTER TABLE idempotency_records DROP CONSTRAINT idempotency_records_operation_check;
ALTER TABLE idempotency_records ADD CONSTRAINT idempotency_records_operation_check
    CHECK (operation IN ('entry.create', 'entry.save', 'content_type.create', 'content_type.update',
                         'changeset.create', 'changeset.publish', 'component.create',
                         'component.update'));
