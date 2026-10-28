-- Milestone M, step M.2: references between entries. docs/modules/01-domain.md, 02-db.md, D-065.
--
-- A reference field names entries of a collection by id. Every revision's references are kept
-- here, written with the revision (revisions never change, so neither do these rows), and the
-- database refuses one that names no entry, or an entry of another collection: the foreign key
-- is on (entry, collection). A draft's references are its draft revision's rows; what readers
-- get references is the published revision's, which the publish transaction checks.
-- Entries are never deleted, so a reference can't come to dangle; if deleting ever arrives, the
-- RESTRICT below makes a referenced entry refuse to go.

CREATE TABLE revision_references (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    revision_id uuid NOT NULL,
    field text NOT NULL CHECK (field ~ '^[a-z][a-z0-9_]{0,63}$'),
    position smallint NOT NULL CHECK (position BETWEEN 0 AND 99),
    collection text NOT NULL,
    entry_id uuid NOT NULL,
    PRIMARY KEY (project_id, environment_id, revision_id, field, position),
    FOREIGN KEY (project_id, environment_id, revision_id)
        REFERENCES revisions (project_id, environment_id, id),
    FOREIGN KEY (project_id, environment_id, entry_id, collection)
        REFERENCES entries (project_id, environment_id, id, collection) ON DELETE RESTRICT
);
-- What names an entry: "referenced by" lists, and the publish and unpublish checks.
CREATE INDEX revision_references_to ON revision_references (project_id, environment_id, entry_id);

ALTER TABLE revision_references ENABLE ROW LEVEL SECURITY;
ALTER TABLE revision_references FORCE ROW LEVEL SECURITY;
CREATE POLICY revision_references_in_project ON revision_references FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);

-- Written once with their revision, never changed.
GRANT SELECT, INSERT ON revision_references TO colophon_api;
