-- Milestone M, step M.5a: content schema migrations — the plan and its items.
-- docs/modules/02-db.md, D-100.
--
-- A migration plan says how one collection's content type moves from the version current when
-- the plan was made (base_version) to a new definition a plain update couldn't apply: the
-- changes a PUT refuses with needs_migration or outright (D-055). Each item names one entry,
-- the draft revision it had at plan time, and the outcome the dry run predicted — the honest
-- "what would happen" the spec's plan step asks for (§12): a dry run is not a reservation
-- (§11), so approve and apply re-check the plan's hash and state, like a review binds a
-- changeset's hash (D-044). Applying starts a run a worker drives item by item (step M.5b);
-- the counters and timestamps it updates are granted below so no later migration re-grants.
--
-- Insert-only in spirit: plans and items change only through the states and columns their
-- lifecycle writes, and nothing is ever deleted. At most one plan per collection may be
-- running, which the unique index enforces so a racing apply loses instead of interleaving.

CREATE TABLE migration_plans (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    id uuid NOT NULL,
    collection text NOT NULL,
    -- The version the plan was computed on; an update that lands first supersedes the plan.
    base_version integer NOT NULL CHECK (base_version >= 1),
    -- The version the apply wrote, once it did.
    target_version integer CHECK (target_version > base_version),
    -- The canonical target definition, kept as text like content_type_versions.
    target_definition text NOT NULL CHECK (octet_length(target_definition) <= 65536),
    target_definition_hash bytea NOT NULL CHECK (length(target_definition_hash) = 32),
    -- The plan-level operations (field moves and fills) the caller declared, as JSON; M.5b acts
    -- on them.
    operations jsonb CHECK (octet_length(operations::text) <= 65536),
    state text NOT NULL DEFAULT 'proposed'
        CHECK (state IN ('proposed', 'approved', 'running', 'done', 'blocked',
                         'superseded', 'cancelled')),
    -- SHA-256 over the collection, base version, target definition hash and operations: what an
    -- approval or an apply is bound to, so it lands only on the plan that was looked at
    -- (D-100).
    plan_hash bytea NOT NULL CHECK (length(plan_hash) = 32),
    created_by uuid NOT NULL REFERENCES identities (id),
    approved_by uuid REFERENCES identities (id),
    approved_at timestamptz,
    -- The dry run's outcome: how many items there are, how many are done, and how many still
    -- block the run (needs_input or blocked).
    expected_items bigint NOT NULL CHECK (expected_items >= 0),
    done_items bigint NOT NULL DEFAULT 0 CHECK (done_items >= 0),
    blocked_items bigint NOT NULL DEFAULT 0 CHECK (blocked_items >= 0),
    started_at timestamptz,
    finished_at timestamptz,
    -- Why a blocked plan stopped, for people.
    error text CHECK (char_length(error) <= 500),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, environment_id, id),
    FOREIGN KEY (project_id, environment_id) REFERENCES environments (project_id, id),
    FOREIGN KEY (project_id, environment_id, collection)
        REFERENCES collections (project_id, environment_id, slug),
    FOREIGN KEY (project_id, environment_id, collection, base_version)
        REFERENCES content_type_versions (project_id, environment_id, collection, version),
    FOREIGN KEY (project_id, environment_id, collection, target_version)
        REFERENCES content_type_versions (project_id, environment_id, collection, version),
    CHECK ((approved_by IS NULL) = (approved_at IS NULL)),
    CHECK (state <> 'approved' OR approved_at IS NOT NULL),
    CHECK (state NOT IN ('running', 'done', 'blocked') OR started_at IS NOT NULL),
    CHECK ((state IN ('done', 'blocked', 'superseded', 'cancelled'))
           = (finished_at IS NOT NULL)),
    CHECK (done_items <= expected_items AND blocked_items <= expected_items)
);

-- At most one running migration per collection: the second apply loses to the index, as a
-- conflict, instead of interleaving its item writes with the first's.
CREATE UNIQUE INDEX migration_plans_one_running
    ON migration_plans (project_id, environment_id, collection) WHERE state = 'running';
-- An environment's plans, newest first; a collection's, and its open ones.
CREATE INDEX migration_plans_by_collection
    ON migration_plans (project_id, environment_id, collection, id DESC);
CREATE INDEX migration_plans_by_state
    ON migration_plans (project_id, environment_id, state, id DESC);
-- A worker's scan for runs to drive, across projects (M.5b).
CREATE INDEX migration_plans_in_flight ON migration_plans (started_at) WHERE state = 'running';

CREATE TABLE migration_items (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    plan_id uuid NOT NULL,
    entry_id uuid NOT NULL,
    -- The draft at plan time: what the apply rewrites.
    expected_revision_id uuid NOT NULL,
    outcome text NOT NULL
        CHECK (outcome IN ('ok', 'migrates', 'needs_input', 'blocked', 'resolved', 'applied',
                           'skipped')),
    -- Why it can't proceed, as a refusal reports it: {"reason", "pointer", "message"}.
    problem jsonb CHECK (octet_length(problem::text) <= 4096),
    -- The replacement document a person gave a needs_input item: content that fits the target,
    -- bound like a revision's content.
    resolution jsonb CHECK (octet_length(resolution::text) <= 263168),
    -- The revision the apply wrote, once it did.
    new_revision_id uuid,
    applied_at timestamptz,
    PRIMARY KEY (project_id, environment_id, plan_id, entry_id),
    FOREIGN KEY (project_id, environment_id, plan_id)
        REFERENCES migration_plans (project_id, environment_id, id),
    FOREIGN KEY (project_id, environment_id, entry_id)
        REFERENCES entries (project_id, environment_id, id),
    FOREIGN KEY (project_id, environment_id, entry_id, expected_revision_id)
        REFERENCES revisions (project_id, environment_id, entry_id, id),
    FOREIGN KEY (project_id, environment_id, entry_id, new_revision_id)
        REFERENCES revisions (project_id, environment_id, entry_id, id),
    CHECK (outcome NOT IN ('needs_input', 'blocked') OR problem IS NOT NULL),
    CHECK (outcome <> 'resolved' OR resolution IS NOT NULL),
    CHECK ((outcome = 'applied') = (new_revision_id IS NOT NULL)),
    CHECK ((new_revision_id IS NULL) = (applied_at IS NULL))
);

-- A plan's items by outcome: the worker's claim scan and the filtered list (M.5b).
CREATE INDEX migration_items_by_outcome
    ON migration_items (project_id, environment_id, plan_id, outcome, entry_id);

-- Nothing new may name a deleted entry, as for pins and publications (migration 0030). A plan
-- is computed under the collection's change lock, so the entries it names are live when it
-- commits; this keeps any later item write to the same rule.
CREATE TRIGGER migration_items_entry_live BEFORE INSERT ON migration_items
    FOR EACH ROW EXECUTE FUNCTION colophon_entry_live();

ALTER TABLE migration_plans ENABLE ROW LEVEL SECURITY;
ALTER TABLE migration_plans FORCE ROW LEVEL SECURITY;
ALTER TABLE migration_items ENABLE ROW LEVEL SECURITY;
ALTER TABLE migration_items FORCE ROW LEVEL SECURITY;

CREATE POLICY migration_plans_in_project ON migration_plans FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
CREATE POLICY migration_items_in_project ON migration_items FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);

-- A worker finds running plans in every project and drives their items (M.5b), like it finds
-- due schedules (0019).
CREATE POLICY migration_plans_for_workers ON migration_plans FOR ALL TO colophon_worker
    USING (true) WITH CHECK (true);
CREATE POLICY migration_items_for_workers ON migration_items FOR ALL TO colophon_worker
    USING (true) WITH CHECK (true);

GRANT SELECT, INSERT,
      UPDATE (state, approved_by, approved_at, expected_items, done_items, blocked_items,
              target_version, started_at, finished_at, error, updated_at)
      ON migration_plans TO colophon_api;
GRANT SELECT, INSERT,
      UPDATE (outcome, problem, resolution, new_revision_id, applied_at)
      ON migration_items TO colophon_api;
-- What a claim and progress updates touch (M.5b): the run's state, counters and times on the
-- plan, and the outcome columns on its items.
GRANT SELECT,
      UPDATE (state, done_items, blocked_items, target_version, started_at, finished_at, error,
              updated_at)
      ON migration_plans TO colophon_worker;
GRANT SELECT,
      UPDATE (outcome, problem, new_revision_id, applied_at)
      ON migration_items TO colophon_worker;

-- Planning, approving, applying, cancelling and resolving are idempotent writes, like every
-- other one.
ALTER TABLE idempotency_records DROP CONSTRAINT idempotency_records_operation_check;
ALTER TABLE idempotency_records ADD CONSTRAINT idempotency_records_operation_check
    CHECK (operation IN ('entry.create', 'entry.save', 'entry.delete', 'entry.import',
                         'entry.localize',
                         'content_type.create', 'content_type.update', 'content_type.delete',
                         'changeset.create', 'changeset.publish',
                         'component.create', 'component.update',
                         'asset.upload', 'asset.delete',
                         'migration.plan', 'migration.approve', 'migration.apply',
                         'migration.cancel', 'migration.resolve'));
