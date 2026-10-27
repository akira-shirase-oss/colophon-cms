-- Scheduled publishing (D-060; spec "Scheduled publishing", §17, case X07). docs/modules/02-db.md,
-- 05-worker.md.
--
-- A schedule publishes one approved changeset at a chosen time. It keeps what its author chose
-- (the local date and time, the IANA time zone, and the choice an ambiguous or skipped local time
-- needed), the instant that resolves to, and the hash and approval it publishes under. When it
-- falls due, a worker publishes it as the person who scheduled it, with every check publishing
-- makes, in the same transaction that marks it published: a crash leaves both or neither, so a
-- schedule publishes at most once. Anything that refuses it (an edit, an expired approval, a role
-- that no longer may publish) blocks it with the reason; nothing is published then.

CREATE TABLE schedules (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    id uuid NOT NULL,
    changeset_id uuid NOT NULL,
    changeset_hash bytea NOT NULL CHECK (length(changeset_hash) = 32),
    approval_id uuid NOT NULL,
    local_time text NOT NULL CHECK (local_time ~ '^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}$'),
    time_zone text NOT NULL CHECK (char_length(time_zone) BETWEEN 1 AND 64),
    choice text CHECK (choice IN ('earlier', 'later')),
    run_at timestamptz NOT NULL,
    state text NOT NULL DEFAULT 'scheduled'
        CHECK (state IN ('scheduled', 'published', 'blocked', 'cancelled')),
    -- Why it was blocked, for people.
    reason text CHECK (char_length(reason) <= 500),
    -- The release it made.
    generation bigint,
    started_at timestamptz,
    finished_at timestamptz,
    scheduled_by uuid NOT NULL REFERENCES identities (id),
    cancelled_by uuid REFERENCES identities (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, environment_id, id),
    FOREIGN KEY (project_id, environment_id, changeset_id)
        REFERENCES changesets (project_id, environment_id, id),
    FOREIGN KEY (project_id, environment_id, approval_id)
        REFERENCES reviews (project_id, environment_id, id),
    CHECK ((state = 'blocked') = (reason IS NOT NULL)),
    CHECK ((state = 'published') = (generation IS NOT NULL)),
    CHECK ((state = 'cancelled') = (cancelled_by IS NOT NULL)),
    CHECK ((state = 'scheduled') = (finished_at IS NULL))
);

-- One pending schedule per changeset.
CREATE UNIQUE INDEX schedules_one_pending ON schedules (project_id, environment_id, changeset_id)
    WHERE state = 'scheduled';
-- What is due, across projects, for workers.
CREATE INDEX schedules_due ON schedules (run_at) WHERE state = 'scheduled';
-- A project's schedules, newest first.
CREATE INDEX schedules_recent ON schedules (project_id, environment_id, created_at DESC);

ALTER TABLE schedules ENABLE ROW LEVEL SECURITY;
ALTER TABLE schedules FORCE ROW LEVEL SECURITY;

CREATE POLICY schedules_in_project ON schedules FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
-- Workers find what is due in every project, and block a schedule whose author can no longer
-- open its project; publishing itself runs as that author, in the project's context.
CREATE POLICY schedules_for_workers ON schedules FOR ALL TO colophon_worker USING (true) WITH CHECK (true);

GRANT SELECT, INSERT,
      UPDATE (state, reason, generation, started_at, finished_at, cancelled_by)
      ON schedules TO colophon_api;
GRANT SELECT (project_id, environment_id, id, run_at, state, scheduled_by),
      UPDATE (state, reason, started_at, finished_at)
      ON schedules TO colophon_worker;
