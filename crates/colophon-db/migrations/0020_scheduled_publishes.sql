-- A publish can be set for a future time instead of now: the schedule pins the changeset's
-- hash, the approval it publishes under, the generation the publisher stood on, and who
-- asked for it. At `run_at` the server claims the row and runs the normal publish — if the
-- world moved (base stale, approval gone, the scheduler lost the role), the commit refuses
-- and `error` says why. A schedule is evidence either way.
CREATE TABLE scheduled_publishes (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    id uuid NOT NULL,
    changeset_id uuid NOT NULL,
    changeset_hash bytea NOT NULL,
    review_id uuid NOT NULL,
    base_generation bigint NOT NULL,
    requested_by uuid NOT NULL,
    -- Not before; if no server runs at that moment it publishes when one is next up.
    run_at timestamptz NOT NULL,
    status text NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'claimed', 'cancelled', 'committed', 'failed')),
    claimed_at timestamptz,
    finished_at timestamptz,
    generation bigint,
    error text CHECK (char_length(error) <= 500),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, environment_id, id),
    FOREIGN KEY (project_id, environment_id, changeset_id)
        REFERENCES changesets (project_id, environment_id, id),
    FOREIGN KEY (project_id, environment_id, review_id)
        REFERENCES reviews (project_id, environment_id, id)
);
-- The due-scan is a partial index: only pending rows ever matter.
CREATE INDEX scheduled_publishes_due ON scheduled_publishes (run_at) WHERE status = 'pending';
COMMENT ON TABLE scheduled_publishes IS 'A publish set for the future: pins the changeset hash, approval and base generation it was scheduled under; the server commits it at run_at or records why it could not.';

ALTER TABLE scheduled_publishes ENABLE ROW LEVEL SECURITY;
ALTER TABLE scheduled_publishes FORCE ROW LEVEL SECURITY;
CREATE POLICY scheduled_in_project ON scheduled_publishes FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
-- The server's claim loop works across projects; app.claim is transaction-local, so the
-- wider policy never reaches a member's request.
CREATE POLICY scheduled_for_claim ON scheduled_publishes FOR ALL TO colophon_api
    USING (current_setting('app.claim', true) = 'on')
    WITH CHECK (current_setting('app.claim', true) = 'on');

-- The API schedules, cancels and reads them; claiming and finishing move the status columns
-- only — the pinned facts of a schedule (changeset, review, base, requester) never change.
GRANT SELECT, INSERT, UPDATE (status, claimed_at, finished_at, generation, error)
    ON scheduled_publishes TO colophon_api;
