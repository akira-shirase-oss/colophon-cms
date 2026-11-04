-- Milestone E, step E2: review stages (Strapi's review workflows) on top of changesets.
-- docs/modules/01-domain.md ("Editorial workflow"), D-104.
--
-- An environment may name an ordered list of review stages; a submitted changeset then
-- publishes only once every stage has an approval on its current hash — a request for changes
-- at any stage blocks the whole changeset. Each review names the stage it decided for, or none
-- for the single implicit review of an environment without stages: with no list configured a
-- review keeps meaning exactly what it did before.
--
-- The list is the environment's live configuration, not pinned into a changeset: renaming or
-- reordering keeps the key its reviews name, so they still count; removing a stage drops its
-- gate and leaves its reviews as history that counts for nothing. A review therefore names a
-- stage by key, never by foreign key — it must outlive the stage it decided for. The order is
-- for people reading a list or a changeset; a stage's approval may be recorded in any order.

CREATE TABLE review_stages (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    -- What a review names it by: a role key's shape.
    key text NOT NULL CHECK (key ~ '^[a-z][a-z0-9-]{0,62}$'),
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 100),
    position integer NOT NULL CHECK (position >= 0),
    -- Its approval must come from someone other than the submitter: spec §06's "projects may
    -- require a different reviewer", per stage.
    different_reviewer boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, environment_id, key),
    UNIQUE (project_id, environment_id, position),
    FOREIGN KEY (project_id, environment_id) REFERENCES environments (project_id, id)
);

-- The stage a review decided for; NULL is the implicit review where none are configured. No
-- foreign key: a review outlives a stage that is removed.
ALTER TABLE reviews
    ADD COLUMN stage text CHECK (stage IS NULL OR stage ~ '^[a-z][a-z0-9-]{0,62}$');

ALTER TABLE review_stages ENABLE ROW LEVEL SECURITY;
ALTER TABLE review_stages FORCE ROW LEVEL SECURITY;
CREATE POLICY review_stages_in_project ON review_stages FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);

-- The list is replaced as a whole — a delete plus the inserts of its new order — and a row is
-- never rewritten in place.
GRANT SELECT, INSERT, DELETE ON review_stages TO colophon_api;

-- Replacing the stage list is idempotent like every other write (D-035).
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
                         'task.create', 'task.update',
                         'stages.update'));
