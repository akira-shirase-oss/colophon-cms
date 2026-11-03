-- Milestone E, step E1: collaboration — comments bound to immutable revisions, and editorial
-- tasks. docs/modules/02-db.md, D-102.
--
-- A comment is written about one revision of one entry and never moves: the composite foreign
-- key on (entry_id, revision_id) keeps it inside the entry it names, and a revision can never
-- change or disappear, so a comment's anchor stays exactly what its author looked at. Its body
-- is kept as typed and is never edited or deleted — the application's grant is SELECT, INSERT
-- and the two resolution columns only, so history keeps every word, and the API serializes the
-- text for the client to escape, never as markup. Resolution is a pair, like a publication's:
-- who resolved it and when, or neither.
--
-- A task is editorial work the API tracks: a title, an optional assignee, a status, and the
-- entry it's about, plus an optional revision and changeset when it's about a particular draft
-- or release candidate. The composite entry key carries the collection like revision_references
-- does (0024), and naming a deleted entry fails the way a new reference, pin or publication
-- does (0030). A task's mutable state is a small set of columns; updating sets done_by and
-- done_at together, so "who closed it" can never be half-recorded.
--
-- Both tables are project-scoped under row-level security like everything else: the same
-- transaction-local context that scopes entries scopes these, so a project can't see or write
-- another's comments or tasks, and a deleted entry's comments stay in the database — only the
-- application role's view of the entry changes (0030).

CREATE TABLE comments (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    id uuid NOT NULL,
    entry_id uuid NOT NULL,
    collection text NOT NULL,
    revision_id uuid NOT NULL,
    -- Where in the revision's content the comment points, recorded in the same path shape
    -- revision_references uses (0025); NULL when the comment is about the revision as a whole.
    field text CHECK (field IS NULL OR (char_length(field) <= 400
        AND field ~ '^[a-z][a-z0-9_]{0,63}(/([0-9]{1,2}|[a-z][a-z0-9_]{0,63})){0,8}$')),
    -- The comment as the author typed it. Rendered escaped; kept verbatim forever.
    body text NOT NULL CHECK (char_length(body) BETWEEN 1 AND 8000),
    author_id uuid NOT NULL REFERENCES identities (id),
    -- Who created the entry, copied at insert so a deleted entry's comments keep their
    -- visibility rule after the application's role can no longer see the entry (0030, D-102).
    entry_created_by uuid NOT NULL REFERENCES identities (id),
    resolved_by uuid REFERENCES identities (id),
    resolved_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, environment_id, id),
    FOREIGN KEY (project_id, environment_id) REFERENCES environments (project_id, id),
    FOREIGN KEY (project_id, environment_id, entry_id, collection)
        REFERENCES entries (project_id, environment_id, id, collection),
    FOREIGN KEY (project_id, environment_id, entry_id, revision_id)
        REFERENCES revisions (project_id, environment_id, entry_id, id),
    CHECK ((resolved_by IS NULL) = (resolved_at IS NULL))
);

-- An entry's comments, newest first (ids are UUID v7); a revision's, for a revision view.
CREATE INDEX comments_by_entry
    ON comments (project_id, environment_id, entry_id, id DESC);
CREATE INDEX comments_by_revision
    ON comments (project_id, environment_id, entry_id, revision_id, id DESC);

-- Nothing new may name a deleted entry, as for references, pins and publications (0030).
CREATE TRIGGER comments_entry_live BEFORE INSERT ON comments
    FOR EACH ROW EXECUTE FUNCTION colophon_entry_live();

CREATE TABLE tasks (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    id uuid NOT NULL,
    entry_id uuid NOT NULL,
    collection text NOT NULL,
    -- The draft the task is about, when it is about one: a revision of the same entry, like a
    -- comment's anchor. NULL names the entry only.
    revision_id uuid,
    -- The changeset the task is about, when it is (E2 builds the review flow on it).
    changeset_id uuid,
    title text NOT NULL CHECK (char_length(title) BETWEEN 1 AND 200),
    -- Who it's waiting on: a member of the project, checked by the API; NULL when unassigned.
    assignee_id uuid REFERENCES identities (id),
    status text NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'done', 'cancelled')),
    created_by uuid NOT NULL REFERENCES identities (id),
    -- Who created the entry, copied at insert like comments': a task stays answerable to the
    -- same read rule after its entry is deleted (0030, D-102).
    entry_created_by uuid NOT NULL REFERENCES identities (id),
    done_by uuid REFERENCES identities (id),
    done_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, environment_id, id),
    FOREIGN KEY (project_id, environment_id) REFERENCES environments (project_id, id),
    FOREIGN KEY (project_id, environment_id, entry_id, collection)
        REFERENCES entries (project_id, environment_id, id, collection),
    FOREIGN KEY (project_id, environment_id, entry_id, revision_id)
        REFERENCES revisions (project_id, environment_id, entry_id, id),
    FOREIGN KEY (project_id, environment_id, changeset_id)
        REFERENCES changesets (project_id, environment_id, id),
    CHECK ((done_by IS NULL) = (done_at IS NULL)),
    CHECK ((status = 'done') = (done_at IS NOT NULL))
);

-- An environment's tasks and the filters the list supports: one entry's, one person's (or the
-- unassigned), one collection's, newest first in each.
CREATE INDEX tasks_by_entry ON tasks (project_id, environment_id, entry_id, id DESC);
CREATE INDEX tasks_by_assignee ON tasks (project_id, environment_id, assignee_id, id DESC);
CREATE INDEX tasks_by_collection ON tasks (project_id, environment_id, collection, id DESC);

CREATE TRIGGER tasks_entry_live BEFORE INSERT ON tasks
    FOR EACH ROW EXECUTE FUNCTION colophon_entry_live();

ALTER TABLE comments ENABLE ROW LEVEL SECURITY;
ALTER TABLE comments FORCE ROW LEVEL SECURITY;
ALTER TABLE tasks ENABLE ROW LEVEL SECURITY;
ALTER TABLE tasks FORCE ROW LEVEL SECURITY;

CREATE POLICY comments_in_project ON comments FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
CREATE POLICY tasks_in_project ON tasks FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);

-- A comment is written once and kept: the API role may insert it, read it, and flip its
-- resolution pair — never rewrite its body or its anchor (D-102).
GRANT SELECT, INSERT, UPDATE (resolved_by, resolved_at) ON comments TO colophon_api;
-- A task changes only in the columns its lifecycle writes.
GRANT SELECT, INSERT,
      UPDATE (title, assignee_id, status, done_by, done_at, updated_at)
      ON tasks TO colophon_api;

-- Collaboration writes are idempotent like every other one (D-035, D-102): commenting,
-- resolving or reopening, and creating or updating a task each record their answer under the
-- caller's key.
ALTER TABLE idempotency_records DROP CONSTRAINT idempotency_records_operation_check;
ALTER TABLE idempotency_records ADD CONSTRAINT idempotency_records_operation_check
    CHECK (operation IN ('entry.create', 'entry.save', 'entry.delete', 'entry.import',
                         'entry.localize',
                         'content_type.create', 'content_type.update', 'content_type.delete',
                         'changeset.create', 'changeset.publish',
                         'component.create', 'component.update',
                         'asset.upload', 'asset.delete',
                         'migration.plan', 'migration.approve', 'migration.apply',
                         'migration.cancel', 'migration.resolve',
                         'comment.create', 'comment.resolve',
                         'task.create', 'task.update'));
