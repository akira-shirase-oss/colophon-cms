-- Milestone P1, step 1.5: content types. docs/modules/02-db.md, 01-domain.md ("Schema model"),
-- D-040.
--
-- A collection is a content type's identity; its definitions are versions, insert-only, each
-- naming the version before it, so the history is a gapless chain and two concurrent edits
-- can't both become version N + 1. Every revision records the collection and the version its
-- content was validated against, and a composite key keeps a revision's collection equal to
-- its entry's.

CREATE TABLE collections (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    slug text NOT NULL CHECK (slug ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, environment_id, slug),
    FOREIGN KEY (project_id, environment_id) REFERENCES environments (project_id, id)
);

CREATE TABLE content_type_versions (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    collection text NOT NULL,
    version integer NOT NULL CHECK (version >= 1),
    previous_version integer,
    -- The canonical definition (ContentType::canonical_json), kept as text with its SHA-256.
    definition text NOT NULL CHECK (octet_length(definition) <= 65536),
    definition_hash bytea NOT NULL CHECK (length(definition_hash) = 32),
    -- NULL for the built-in definitions this migration and project creation write.
    created_by uuid REFERENCES identities (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, environment_id, collection, version),
    FOREIGN KEY (project_id, environment_id, collection)
        REFERENCES collections (project_id, environment_id, slug),
    FOREIGN KEY (project_id, environment_id, collection, previous_version)
        REFERENCES content_type_versions (project_id, environment_id, collection, version),
    CHECK ((version = 1 AND previous_version IS NULL) OR previous_version = version - 1)
);

-- The backfill below must see every row, and production's migration login is not a
-- superuser; FORCE comes back on at the end.
ALTER TABLE entries NO FORCE ROW LEVEL SECURITY;
ALTER TABLE revisions NO FORCE ROW LEVEL SECURITY;

-- Every environment starts with the built-in articles type, version 1: exactly
-- ContentType::articles().canonical_json(), which a test holds this text to.
INSERT INTO collections (project_id, environment_id, slug)
SELECT project_id, id, 'articles' FROM environments;
INSERT INTO content_type_versions (project_id, environment_id, collection, version, definition, definition_hash)
SELECT project_id, id, 'articles', 1, definition, sha256(convert_to(definition, 'UTF8'))
FROM environments,
     (VALUES ('{"name":"Articles","title_field":"title","fields":[{"id":"title","label":"Title","type":"text","required":true,"max_chars":200,"multiline":false},{"id":"body","label":"Body","type":"rich_text","required":true}]}'))
         AS builtin (definition);

-- Entries belong to a collection that exists.
ALTER TABLE entries DROP CONSTRAINT entries_collection_check;
ALTER TABLE entries ADD FOREIGN KEY (project_id, environment_id, collection)
    REFERENCES collections (project_id, environment_id, slug);
ALTER TABLE entries ADD UNIQUE (project_id, environment_id, id, collection);

-- Revisions record their collection and schema version; every existing one is an article.
ALTER TABLE revisions ADD COLUMN collection text, ADD COLUMN schema_version integer;
UPDATE revisions r SET collection = e.collection, schema_version = 1
FROM entries e
WHERE e.project_id = r.project_id AND e.environment_id = r.environment_id AND e.id = r.entry_id;
ALTER TABLE revisions ALTER COLUMN collection SET NOT NULL, ALTER COLUMN schema_version SET NOT NULL;
ALTER TABLE revisions ADD FOREIGN KEY (project_id, environment_id, entry_id, collection)
    REFERENCES entries (project_id, environment_id, id, collection);
ALTER TABLE revisions ADD FOREIGN KEY (project_id, environment_id, collection, schema_version)
    REFERENCES content_type_versions (project_id, environment_id, collection, version);

ALTER TABLE entries FORCE ROW LEVEL SECURITY;
ALTER TABLE revisions FORCE ROW LEVEL SECURITY;

-- Content type writes are idempotent too.
ALTER TABLE idempotency_records DROP CONSTRAINT idempotency_records_operation_check;
ALTER TABLE idempotency_records ADD CONSTRAINT idempotency_records_operation_check
    CHECK (operation IN ('entry.create', 'entry.save', 'content_type.create', 'content_type.update'));

ALTER TABLE collections ENABLE ROW LEVEL SECURITY;
ALTER TABLE collections FORCE ROW LEVEL SECURITY;
ALTER TABLE content_type_versions ENABLE ROW LEVEL SECURITY;
ALTER TABLE content_type_versions FORCE ROW LEVEL SECURITY;
CREATE POLICY collections_in_project ON collections FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
CREATE POLICY content_type_versions_in_project ON content_type_versions FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);

-- Versions are insert-only, like revisions.
GRANT SELECT, INSERT ON collections TO colophon_api;
GRANT SELECT, INSERT ON content_type_versions TO colophon_api;
