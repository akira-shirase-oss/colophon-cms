-- Bounded bulk work on drafts (P5, D-116): a manifest of exact entries and base revisions,
-- chunked at most 100 items a request, driven by the worker as the identity that submitted
-- it, one transaction per item. A job is never globally atomic: every item answers applied,
-- conflict, refused or skipped on its own, and a crash mid-run resumes on the rows that
-- stayed pending. The route family is {P}/bulk because {P}/jobs is delivery's.

CREATE TABLE bulk_jobs (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    id uuid NOT NULL,
    -- What the job operates on. 'entries' is the only family for now: draft writes and
    -- deletes on the entries of one collection, named in `operation`.
    kind text NOT NULL CHECK (kind = 'entries'),
    -- The job's operation document: for 'entries', {"collection": "<slug>"}. Kept as a
    -- document so a later family can carry its own parameters; bounded like a definition.
    operation jsonb NOT NULL CHECK (octet_length(operation::text) <= 65536),
    -- SHA-256 that identifies this manifest: the job head, then every appended item's
    -- canonical form rolled in as its chunk lands. If-Match binds to it.
    manifest_hash bytea NOT NULL CHECK (length(manifest_hash) = 32),
    state text NOT NULL DEFAULT 'accepting'
        CHECK (state IN ('accepting', 'running', 'done', 'blocked', 'cancelled')),
    created_by uuid NOT NULL REFERENCES identities (id),
    -- Whether `created_by` signs in as a member or is a project API token's own identity:
    -- the worker reopens each item's transaction through the matching path so a revoked
    -- token or a demoted member stops the run honestly (D-116).
    principal text NOT NULL CHECK (principal IN ('member', 'token')),
    -- The manifest's size and progress, recomputed at every checkpoint.
    expected_items bigint NOT NULL CHECK (expected_items >= 0),
    done_items bigint NOT NULL DEFAULT 0 CHECK (done_items >= 0),
    blocked_items bigint NOT NULL DEFAULT 0 CHECK (blocked_items >= 0),
    started_at timestamptz,
    finished_at timestamptz,
    -- Why a blocked or cancelled job stopped, for people.
    error text CHECK (char_length(error) <= 500),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, environment_id, id),
    UNIQUE (id),
    FOREIGN KEY (project_id, environment_id) REFERENCES environments (project_id, id),
    CHECK ((state = 'accepting') = (started_at IS NULL)),
    CHECK ((state IN ('done', 'blocked', 'cancelled')) = (finished_at IS NOT NULL))
);

COMMENT ON TABLE bulk_jobs IS
    'One bulk manifest and its progress: accepting while chunks land, running once sealed, then done, blocked or cancelled (P5, D-116).';
COMMENT ON COLUMN bulk_jobs.manifest_hash IS
    'SHA-256 rolled over the job head and each appended chunk: the If-Match identity of the manifest (D-116).';
COMMENT ON COLUMN bulk_jobs.principal IS
    'member or token: which scope path the worker opens each item transaction through, so revocation stops the run (D-116).';

-- The jobs a worker can still drive, like migration_plans_in_flight (0040).
CREATE INDEX bulk_jobs_in_flight
    ON bulk_jobs (project_id, environment_id, started_at) WHERE state = 'running';

CREATE TABLE bulk_items (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    job_id uuid NOT NULL,
    -- The caller's stable name for this logical item, unique within the job: a retried
    -- append lands once, and the resumable cursor walks this order.
    key text NOT NULL CHECK (key <> '' AND char_length(key) <= 256),
    entry_id uuid NOT NULL,
    expected_revision_id uuid NOT NULL,
    -- The item's operation document: {"op": "update"|"set"|"move"|"delete", ...}, bounded.
    input jsonb NOT NULL CHECK (octet_length(input::text) <= 65536),
    outcome text NOT NULL DEFAULT 'pending'
        CHECK (outcome IN ('pending', 'applied', 'conflict', 'refused', 'skipped')),
    -- Why it didn't apply, as a refusal reports it: {"reason", "pointer", "message"}.
    problem jsonb CHECK (octet_length(problem::text) <= 4096),
    new_revision_id uuid,
    applied_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, environment_id, job_id, key),
    FOREIGN KEY (project_id, environment_id, job_id)
        REFERENCES bulk_jobs (project_id, environment_id, id),
    FOREIGN KEY (project_id, environment_id, entry_id)
        REFERENCES entries (project_id, environment_id, id),
    FOREIGN KEY (project_id, environment_id, entry_id, expected_revision_id)
        REFERENCES revisions (project_id, environment_id, entry_id, id),
    FOREIGN KEY (project_id, environment_id, entry_id, new_revision_id)
        REFERENCES revisions (project_id, environment_id, entry_id, id),
    -- Terminal rows are coherent: an applied item stamps its time (deletes write no
    -- revision), a new revision only ever comes from an apply, and a problem explains a
    -- conflict or refusal.
    CHECK ((outcome = 'applied') = (applied_at IS NOT NULL)),
    CHECK (new_revision_id IS NULL OR outcome = 'applied'),
    CHECK (problem IS NULL OR outcome IN ('conflict', 'refused', 'skipped'))
);

COMMENT ON TABLE bulk_items IS
    'One manifest item: exact entry and base revision, its operation, and its own outcome (P5, D-116).';

-- A job's items by outcome: the worker's claim scan and the filtered list.
CREATE INDEX bulk_items_by_outcome
    ON bulk_items (project_id, environment_id, job_id, outcome, key);

-- Nothing new may name a deleted entry, like migration items (0040).
CREATE TRIGGER bulk_items_entry_live BEFORE INSERT ON bulk_items
    FOR EACH ROW EXECUTE FUNCTION colophon_entry_live();

ALTER TABLE bulk_jobs ENABLE ROW LEVEL SECURITY;
ALTER TABLE bulk_jobs FORCE ROW LEVEL SECURITY;
ALTER TABLE bulk_items ENABLE ROW LEVEL SECURITY;
ALTER TABLE bulk_items FORCE ROW LEVEL SECURITY;

CREATE POLICY bulk_jobs_in_project ON bulk_jobs FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
CREATE POLICY bulk_items_in_project ON bulk_items FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);

-- A worker finds running jobs in every project and drives their items (P5), like it finds
-- running plans (0040) and due schedules (0019).
CREATE POLICY bulk_jobs_for_workers ON bulk_jobs FOR ALL TO colophon_worker
    USING (true) WITH CHECK (true);
CREATE POLICY bulk_items_for_workers ON bulk_items FOR ALL TO colophon_worker
    USING (true) WITH CHECK (true);

GRANT SELECT, INSERT,
      UPDATE (manifest_hash, state, expected_items, done_items, blocked_items,
              started_at, finished_at, error, updated_at)
      ON bulk_jobs TO colophon_api;
GRANT SELECT, INSERT,
      UPDATE (outcome, problem, new_revision_id, applied_at)
      ON bulk_items TO colophon_api;
GRANT SELECT,
      UPDATE (state, expected_items, done_items, blocked_items, started_at, finished_at,
              error, updated_at)
      ON bulk_jobs TO colophon_worker;
GRANT SELECT,
      UPDATE (outcome, problem, new_revision_id, applied_at)
      ON bulk_items TO colophon_worker;

-- Submitting, appending and cancelling are idempotent writes, like every write route.
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
                         'stages.update',
                         'bulk.submit', 'bulk.items', 'bulk.cancel'));

-- Bulk work notifies under 'releases', the existing jobs kind (0044): a client listening
-- for release-shaped work hears a manifest change too; no new events::Kind (D-116).
CREATE TRIGGER bulk_jobs_notify AFTER INSERT OR UPDATE OR DELETE ON bulk_jobs
    FOR EACH ROW EXECUTE FUNCTION colophon_notify('releases', '');
CREATE TRIGGER bulk_items_notify AFTER INSERT OR UPDATE OR DELETE ON bulk_items
    FOR EACH ROW EXECUTE FUNCTION colophon_notify('releases', '');

-- A bulk job holds budget reservations like a delivery job does (0045): one 'jobs' slot for
-- the job itself and one 'documents' unit per manifest item, linked here so the ledger can
-- tell them from delivery rows. At most one job linkage per row.
ALTER TABLE budget_reservations ADD COLUMN bulk_job_id uuid;
ALTER TABLE budget_reservations ADD CONSTRAINT budget_reservations_bulk_job_fkey
    FOREIGN KEY (bulk_job_id) REFERENCES bulk_jobs (id);
ALTER TABLE budget_reservations ADD CONSTRAINT budget_reservations_one_job_check
    CHECK (num_nonnulls(job_id, bulk_job_id) <= 1);
ALTER TABLE budget_reservations ADD CONSTRAINT budget_reservations_bulk_kind_check
    CHECK (bulk_job_id IS NULL OR kind IN ('documents', 'jobs'));
CREATE INDEX budget_reservations_by_bulk_job
    ON budget_reservations (bulk_job_id) WHERE bulk_job_id IS NOT NULL AND state IN ('held', 'uncertain');
