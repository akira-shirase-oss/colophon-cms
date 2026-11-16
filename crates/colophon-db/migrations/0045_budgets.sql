-- Milestone P5, step P5.2: the budget substrate (docs/modules/09-automation.md, "Budgets";
-- spec §17, §33; D-113, invariant AU3).
--
-- A principal's limits live in `api_token_limits`: hard ceilings the accepting transaction
-- enforces, one row per API token. A limit that is absent is unlimited at this layer — the
-- Valkey GCRA limiter still shapes request rate (D-076); these rows are the hard quota
-- authority, not a rate limiter.
--
-- `budget_reservations` is the ledger: a conservative allowance taken in the transaction that
-- accepts the work, held while the work is in flight, settled with what the outcome actually
-- consumed in the same commit that records it, released when the work can never happen, and
-- marked `uncertain` by the worker when a held row outlives the work it could have paid for —
-- kept, never silently dropped, and announced as `budget.discrepancy` in the audit log.
-- Counting is a locked-counter model: `reserve` locks the principal's limits row FOR UPDATE
-- inside the accepting transaction, so two concurrent requests of one token can never both
-- see room that isn't there.

-- The actor the worker's `budget.discrepancy` events name: no person acts, so the audit log
-- says so (its own issuer, like a token's, so nothing can sign in as it).
INSERT INTO identities (id, issuer, subject, display_name)
VALUES ('029cfcbb-de9b-48c9-94fb-8bf6b5a3502a', 'urn:colophon:system', 'worker', 'Colophon worker');

-- Per-token ceilings. Every column NULLable: NULL means that dimension is unconstrained.
-- A row with all NULL would mean "limited to nothing", which the CHECK refuses; removing a
-- token's limits deletes the row.
CREATE TABLE api_token_limits (
    project_id uuid NOT NULL,
    token_id uuid NOT NULL,
    -- Entry writes the token's requests commit per rolling day: creates, saves, localizations,
    -- import items and the mirror writes their references cause (D-113).
    documents_per_day bigint CHECK (documents_per_day BETWEEN 1 AND 1000000),
    -- Delivery jobs the token's requests may hold at once (held or uncertain reservations).
    max_concurrent_jobs bigint CHECK (max_concurrent_jobs BETWEEN 1 AND 64),
    -- Management-plane requests per minute: stored for the hard quota slot; the GCRA limiter
    -- still does the shaping and nothing consumes this yet (D-113).
    requests_per_minute bigint CHECK (requests_per_minute BETWEEN 1 AND 100000),
    updated_by uuid NOT NULL REFERENCES identities (id),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, token_id),
    FOREIGN KEY (project_id, token_id) REFERENCES api_tokens (project_id, id),
    CHECK (num_nonnulls(documents_per_day, max_concurrent_jobs, requests_per_minute) >= 1)
);
CREATE TRIGGER api_token_limits_notify AFTER INSERT OR UPDATE OR DELETE ON api_token_limits
    FOR EACH ROW EXECUTE FUNCTION colophon_notify('tokens', '');

ALTER TABLE api_token_limits ENABLE ROW LEVEL SECURITY;
ALTER TABLE api_token_limits FORCE ROW LEVEL SECURITY;
CREATE POLICY api_token_limits_in_project ON api_token_limits FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
CREATE POLICY api_token_limits_for_workers ON api_token_limits FOR SELECT TO colophon_worker
    USING (true);

GRANT SELECT, INSERT, UPDATE, DELETE ON api_token_limits TO colophon_api;
GRANT SELECT ON api_token_limits TO colophon_worker;

CREATE TABLE budget_reservations (
    project_id uuid NOT NULL REFERENCES projects (id),
    -- Today's kinds are all environment-scoped work (entry writes, delivery jobs).
    environment_id uuid NOT NULL,
    id uuid NOT NULL,
    -- Who the work is for: the principal that set it in motion (a token's identity today).
    principal_id uuid NOT NULL REFERENCES identities (id),
    kind text NOT NULL CHECK (kind IN ('documents', 'jobs', 'requests', 'tokens', 'cost')),
    -- The conservative allowance taken at accept time: writes for `documents`, job slots for
    -- `jobs`; provider units for the `tokens`/`cost` kinds the assistant would use.
    reserved bigint NOT NULL CHECK (reserved >= 0),
    -- What the outcome actually consumed, recorded with it; NULL while the work is in flight.
    settled bigint CHECK (settled IS NULL OR (settled >= 0 AND settled <= reserved)),
    -- `held` in flight; `settled` consumed; `released` never happened; `uncertain` held past
    -- what could still be running — retained until a reconciler or a person resolves it.
    state text NOT NULL CHECK (state IN ('held', 'settled', 'released', 'uncertain')),
    -- The delivery job the reservation pays for, when there is one.
    job_id uuid,
    created_at timestamptz NOT NULL DEFAULT now(),
    -- When the outcome landed, or the reservation was freed (`settled` and `released` only).
    settled_at timestamptz,
    PRIMARY KEY (project_id, id),
    FOREIGN KEY (project_id, environment_id) REFERENCES environments (project_id, id),
    FOREIGN KEY (project_id, environment_id, job_id) REFERENCES jobs (project_id, environment_id, id),
    -- The state machine: only settled carries what was consumed; only outcomes carry a time.
    CHECK (state <> 'held' OR (settled IS NULL AND settled_at IS NULL)),
    CHECK (state <> 'uncertain' OR (settled IS NULL AND settled_at IS NULL)),
    CHECK (state <> 'settled' OR (settled IS NOT NULL AND settled_at IS NOT NULL)),
    CHECK (state <> 'released' OR (settled IS NULL AND settled_at IS NOT NULL)),
    CHECK (job_id IS NULL OR kind = 'jobs')
);

-- The accepting transaction's count: what a principal holds or spent, per kind and window.
CREATE INDEX budget_reservations_by_principal
    ON budget_reservations (principal_id, kind, state, created_at DESC);
-- The reconciler's scan: held reservations by age.
CREATE INDEX budget_reservations_held ON budget_reservations (created_at) WHERE state = 'held';
-- A job's open reservation, for the outcome that closes it and the release that supersedes it.
CREATE INDEX budget_reservations_by_job
    ON budget_reservations (job_id) WHERE job_id IS NOT NULL AND state IN ('held', 'uncertain');

ALTER TABLE budget_reservations ENABLE ROW LEVEL SECURITY;
ALTER TABLE budget_reservations FORCE ROW LEVEL SECURITY;
CREATE POLICY budget_reservations_in_project ON budget_reservations FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
-- The worker sees every project's reservations: it reconciles what no request is left to fix.
CREATE POLICY budget_reservations_for_workers ON budget_reservations FOR ALL TO colophon_worker
    USING (true) WITH CHECK (true);

GRANT SELECT, INSERT, UPDATE (state, settled, settled_at) ON budget_reservations TO colophon_api;
GRANT SELECT, UPDATE (state, settled, settled_at) ON budget_reservations TO colophon_worker;

-- The reconciler writes `budget.discrepancy` events as the system identity, and nothing else.
CREATE POLICY audit_events_worker_discrepancy ON audit_events FOR INSERT TO colophon_worker
    WITH CHECK (action = 'budget.discrepancy'
                AND actor_id = '029cfcbb-de9b-48c9-94fb-8bf6b5a3502a');
GRANT INSERT ON audit_events TO colophon_worker;
