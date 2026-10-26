-- Milestone P1, step 1.2: entries, their immutable revisions, and idempotency records.
-- docs/modules/02-db.md ("Transactions", "Constraints that carry invariants"), D-034, D-035.
--
-- An entry's draft pointer names one of its own revisions, and a revision's base is an earlier
-- revision of the same entry. Both are composite foreign keys, so neither can cross an entry,
-- an environment or a project (D3). The application may insert revisions but never change or
-- delete one (D1). A save moves the pointer only while it still names the revision the editor
-- started from (D2).

CREATE TABLE entries (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    id uuid NOT NULL,
    collection text NOT NULL CHECK (collection IN ('articles')),
    draft_revision_id uuid NOT NULL,
    created_by uuid NOT NULL REFERENCES identities (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, environment_id, id),
    FOREIGN KEY (project_id, environment_id) REFERENCES environments (project_id, id)
);
-- Lists: newest first within a collection (ids are UUID v7, so they sort by creation).
CREATE INDEX entries_by_collection ON entries (project_id, environment_id, collection, id);

CREATE TABLE revisions (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    entry_id uuid NOT NULL,
    id uuid NOT NULL,
    base_revision_id uuid CHECK (base_revision_id <> id),
    -- The article's canonical JSON, kept as text: its bytes, and so its hash, never change.
    content text NOT NULL CHECK (octet_length(content) <= 263168),
    content_hash bytea NOT NULL CHECK (length(content_hash) = 32),
    -- Copied from the content for lists, which shouldn't parse every document.
    title text NOT NULL CHECK (char_length(title) BETWEEN 1 AND 200),
    author_id uuid NOT NULL REFERENCES identities (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, environment_id, id),
    -- Also serves history: one entry's revisions, newest first.
    UNIQUE (project_id, environment_id, entry_id, id),
    FOREIGN KEY (project_id, environment_id, entry_id)
        REFERENCES entries (project_id, environment_id, id),
    FOREIGN KEY (project_id, environment_id, entry_id, base_revision_id)
        REFERENCES revisions (project_id, environment_id, entry_id, id)
);

-- Checked at commit: an entry and its first revision are created in one transaction.
ALTER TABLE entries ADD CONSTRAINT entries_draft_is_own_revision
    FOREIGN KEY (project_id, environment_id, id, draft_revision_id)
    REFERENCES revisions (project_id, environment_id, entry_id, id)
    DEFERRABLE INITIALLY DEFERRED;

-- One record per (who, where, what, key). It is claimed before the work and completed with the
-- response in the same transaction, so a retry after a lost response gets the same answer
-- (F02), and a concurrent duplicate waits for the first and then replays it. Only successes are
-- recorded: a failed attempt rolls its claim back and can simply be retried.
CREATE TABLE idempotency_records (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    identity_id uuid NOT NULL REFERENCES identities (id),
    operation text NOT NULL CHECK (operation IN ('entry.create', 'entry.save')),
    key text NOT NULL CHECK (key ~ '^[A-Za-z0-9_-]{16,128}$'),
    request_hash bytea NOT NULL CHECK (length(request_hash) = 32),
    -- Set in the claiming transaction before it commits.
    status smallint CHECK (status BETWEEN 200 AND 299),
    response text CHECK (octet_length(response) <= 300000),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, environment_id, identity_id, operation, key),
    FOREIGN KEY (project_id, environment_id) REFERENCES environments (project_id, id),
    CHECK ((status IS NULL) = (response IS NULL))
);
-- For the retention job (keys live at least 7 days; module 02).
CREATE INDEX idempotency_records_by_age ON idempotency_records (created_at);

ALTER TABLE entries ENABLE ROW LEVEL SECURITY;
ALTER TABLE entries FORCE ROW LEVEL SECURITY;
ALTER TABLE revisions ENABLE ROW LEVEL SECURITY;
ALTER TABLE revisions FORCE ROW LEVEL SECURITY;
ALTER TABLE idempotency_records ENABLE ROW LEVEL SECURITY;
ALTER TABLE idempotency_records FORCE ROW LEVEL SECURITY;

CREATE POLICY entries_in_project ON entries FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
CREATE POLICY revisions_in_project ON revisions FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
-- A person sees only their own idempotency records.
CREATE POLICY idempotency_records_own ON idempotency_records FOR ALL TO colophon_api
    USING (
        project_id = nullif(current_setting('app.project_id', true), '')::uuid
        AND identity_id = nullif(current_setting('app.identity_id', true), '')::uuid
    )
    WITH CHECK (
        project_id = nullif(current_setting('app.project_id', true), '')::uuid
        AND identity_id = nullif(current_setting('app.identity_id', true), '')::uuid
    );

-- Revisions: insert-only (D1). Entries: only the draft pointer moves.
GRANT SELECT, INSERT, UPDATE (draft_revision_id, updated_at) ON entries TO colophon_api;
GRANT SELECT, INSERT ON revisions TO colophon_api;
GRANT SELECT, INSERT, UPDATE (status, response) ON idempotency_records TO colophon_api;
