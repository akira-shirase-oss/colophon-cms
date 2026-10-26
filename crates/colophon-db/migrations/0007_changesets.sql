-- Milestone P2, step 2.1: changesets and their review. docs/modules/01-domain.md ("Editorial
-- workflow"), D-044.
--
-- A changeset pins exact revisions of entries in one environment. Each revision row already
-- carries its content hash, collection and content-type version, so pinning the revision pins
-- them all, and the changeset's hash (colophon_domain::changeset::canonical, SHA-256) can always
-- be recomputed from a join. Refreshing a changeset after an edit re-pins the new revisions and
-- stores the new hash.
--
-- Reviews are insert-only: each records the hash it decided on, and counts only while the
-- changeset still has that hash. The application checks the hash under a lock on the changeset
-- row before inserting, and refreshes take the same lock, so a review can't land on a changeset
-- that changed under it. Under that lock each review also takes the changeset's next sequence
-- number: the order decisions were recorded in, which timestamps can't give (two can share one,
-- and a transaction that waited for the lock started before the one it waited for).

CREATE TABLE changesets (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    id uuid NOT NULL,
    title text NOT NULL CHECK (char_length(title) BETWEEN 1 AND 200),
    hash bytea NOT NULL CHECK (length(hash) = 32),
    created_by uuid NOT NULL REFERENCES identities (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    submitted_by uuid REFERENCES identities (id),
    submitted_at timestamptz,
    updated_at timestamptz NOT NULL DEFAULT now(),
    CHECK ((submitted_by IS NULL) = (submitted_at IS NULL)),
    PRIMARY KEY (project_id, environment_id, id),
    FOREIGN KEY (project_id, environment_id) REFERENCES environments (project_id, id)
);

CREATE TABLE changeset_items (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    changeset_id uuid NOT NULL,
    entry_id uuid NOT NULL,
    revision_id uuid NOT NULL,
    PRIMARY KEY (project_id, environment_id, changeset_id, entry_id),
    FOREIGN KEY (project_id, environment_id, changeset_id)
        REFERENCES changesets (project_id, environment_id, id),
    FOREIGN KEY (project_id, environment_id, entry_id, revision_id)
        REFERENCES revisions (project_id, environment_id, entry_id, id)
);

CREATE TABLE reviews (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    id uuid NOT NULL,
    changeset_id uuid NOT NULL,
    changeset_hash bytea NOT NULL CHECK (length(changeset_hash) = 32),
    seq integer NOT NULL CHECK (seq >= 1),
    reviewer_id uuid NOT NULL REFERENCES identities (id),
    decision text NOT NULL CHECK (decision IN ('approve', 'request_changes')),
    comment text NOT NULL DEFAULT '' CHECK (char_length(comment) <= 2000),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, environment_id, id),
    -- Also serves "a changeset's decisions, newest first".
    UNIQUE (project_id, environment_id, changeset_id, seq),
    FOREIGN KEY (project_id, environment_id, changeset_id)
        REFERENCES changesets (project_id, environment_id, id)
);

ALTER TABLE changesets ENABLE ROW LEVEL SECURITY;
ALTER TABLE changesets FORCE ROW LEVEL SECURITY;
ALTER TABLE changeset_items ENABLE ROW LEVEL SECURITY;
ALTER TABLE changeset_items FORCE ROW LEVEL SECURITY;
ALTER TABLE reviews ENABLE ROW LEVEL SECURITY;
ALTER TABLE reviews FORCE ROW LEVEL SECURITY;
CREATE POLICY changesets_in_project ON changesets FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
CREATE POLICY changeset_items_in_project ON changeset_items FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
CREATE POLICY reviews_in_project ON reviews FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);

GRANT SELECT, INSERT, UPDATE (hash, submitted_by, submitted_at, updated_at) ON changesets TO colophon_api;
GRANT SELECT, INSERT, UPDATE (revision_id) ON changeset_items TO colophon_api;
GRANT SELECT, INSERT ON reviews TO colophon_api;

-- Creating a changeset is idempotent, like creating an entry.
ALTER TABLE idempotency_records DROP CONSTRAINT idempotency_records_operation_check;
ALTER TABLE idempotency_records ADD CONSTRAINT idempotency_records_operation_check
    CHECK (operation IN ('entry.create', 'entry.save', 'content_type.create', 'content_type.update', 'changeset.create'));
