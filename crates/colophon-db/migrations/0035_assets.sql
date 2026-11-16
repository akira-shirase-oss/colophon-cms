-- Milestone P4, media step 0: uploads with their own lifecycle. docs/modules/07-media.md.
--
-- An upload lands in quarantine: the API streams the client's bytes to a random object key,
-- records the asset and its verify job in one transaction, and answers "accepted" — truthful,
-- because nothing is usable yet. The worker then downloads the object, checks the real file
-- signature (the declared MIME type proves nothing, M2), decodes it under bounds, and either
-- promotes the object to its public key and the asset to `usable`, or marks it `rejected`
-- with a safe reason. Public keys exist only for usable assets, so a checked file is the only
-- kind an origin can ever serve (M1, M5).
--
-- asset_jobs borrows the jobs table's lease discipline (D-046): FOR UPDATE SKIP LOCKED claims,
-- a lease owner/expiry/generation, and finalizing through a conditional update so a late
-- attempt from an expired lease is refused. Kinds grow with the lifecycle: 'verify' now;
-- 'derive', 'expire' and 'delete' when retention lands (P6).

CREATE TABLE assets (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    id uuid NOT NULL,
    -- The lifecycle: quarantined while the worker decides, then exactly one of usable/rejected.
    state text NOT NULL DEFAULT 'quarantined'
        CHECK (state IN ('quarantined', 'usable', 'rejected')),
    -- What the client called it; a label, never the storage name.
    file_name text NOT NULL CHECK (char_length(file_name) BETWEEN 1 AND 200),
    -- What the client claimed (untrusted, kept for diagnosis) and what the signature proved.
    declared_type text NOT NULL CHECK (char_length(declared_type) <= 100),
    detected_type text CHECK (char_length(detected_type) <= 100),
    size_bytes bigint NOT NULL CHECK (size_bytes BETWEEN 1 AND 20971520),
    -- Set at verification: the content's digest and the decoded dimensions.
    sha256 bytea,
    width integer CHECK (width IS NULL OR width > 0),
    height integer CHECK (height IS NULL OR height > 0),
    -- Object-store keys: a random quarantine name, then a stable public name once usable.
    quarantine_key text NOT NULL UNIQUE,
    public_key text UNIQUE,
    rejected_reason text CHECK (char_length(rejected_reason) <= 200),
    created_by uuid NOT NULL REFERENCES identities (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    verified_at timestamptz,
    PRIMARY KEY (project_id, environment_id, id),
    FOREIGN KEY (project_id, environment_id) REFERENCES environments (project_id, id)
);

CREATE TABLE asset_jobs (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    id uuid NOT NULL,
    asset_id uuid NOT NULL,
    kind text NOT NULL CHECK (kind IN ('verify')),
    idempotency_key text NOT NULL UNIQUE,
    status text NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'running', 'retrying', 'succeeded', 'failed', 'superseded')),
    attempts integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    lease_owner text CHECK (char_length(lease_owner) <= 200),
    lease_expires_at timestamptz,
    lease_generation integer NOT NULL DEFAULT 0,
    next_attempt_at timestamptz NOT NULL DEFAULT now(),
    deadline_at timestamptz NOT NULL,
    last_error text CHECK (char_length(last_error) <= 500),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    finished_at timestamptz,
    PRIMARY KEY (project_id, environment_id, id),
    FOREIGN KEY (project_id, environment_id, asset_id)
        REFERENCES assets (project_id, environment_id, id)
);

CREATE INDEX asset_jobs_due ON asset_jobs (next_attempt_at)
    WHERE status IN ('pending', 'retrying');

ALTER TABLE assets ENABLE ROW LEVEL SECURITY;
ALTER TABLE assets FORCE ROW LEVEL SECURITY;
ALTER TABLE asset_jobs ENABLE ROW LEVEL SECURITY;
ALTER TABLE asset_jobs FORCE ROW LEVEL SECURITY;
CREATE POLICY assets_in_project ON assets FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
CREATE POLICY asset_jobs_in_project ON asset_jobs FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);

-- The API records uploads and their verify jobs; the worker moves assets and claims the work.
GRANT SELECT, INSERT ON assets TO colophon_api;
GRANT SELECT, INSERT ON asset_jobs TO colophon_api;
CREATE POLICY asset_jobs_for_workers ON asset_jobs FOR ALL TO colophon_worker USING (true) WITH CHECK (true);
CREATE POLICY assets_for_workers ON assets FOR ALL TO colophon_worker USING (true) WITH CHECK (true);
GRANT SELECT, UPDATE (status, attempts, lease_owner, lease_expires_at, lease_generation, next_attempt_at,
                      last_error, updated_at, finished_at) ON asset_jobs TO colophon_worker;
GRANT SELECT, UPDATE (state, detected_type, sha256, width, height, public_key, rejected_reason,
                      verified_at) ON assets TO colophon_worker;

-- The upload claims its idempotency key like every other write.
ALTER TABLE idempotency_records DROP CONSTRAINT idempotency_records_operation_check;
ALTER TABLE idempotency_records ADD CONSTRAINT idempotency_records_operation_check
    CHECK (operation IN ('entry.create', 'entry.save', 'entry.delete', 'entry.import',
                         'entry.localize',
                         'content_type.create', 'content_type.update', 'content_type.delete',
                         'changeset.create', 'changeset.publish',
                         'component.create', 'component.update',
                         'asset.upload'));
