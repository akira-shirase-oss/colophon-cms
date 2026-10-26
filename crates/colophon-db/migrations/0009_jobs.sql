-- Milestone P2, step 2.3: work that survives a crash. docs/modules/05-worker.md, D-046.
--
-- Destinations are where a project's releases are announced (a webhook for now). Committing a
-- release inserts one job per enabled destination in the same transaction: the transactional
-- outbox, so a committed release can't lose its deliveries and an uncommitted one can't send
-- any. The same insert supersedes older jobs for that destination that haven't started.
--
-- Workers claim due jobs with FOR UPDATE SKIP LOCKED, under a lease (owner, expiry and a lease
-- generation that goes up with every claim), and finalize only through a conditional update on
-- that lease, so a late result from an expired lease is refused. An expired lease is claimed
-- again, and the attempt repeats with the job's stable idempotency key: whether the first one
-- reached the destination is unknown, and the key lets the destination drop the duplicate.
--
-- The worker runs as colophon_worker (deploy/postgres/roles.sql), which sees jobs in every
-- project but nothing else of them beyond destinations and the releases being announced.

CREATE TABLE destinations (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    id uuid NOT NULL,
    kind text NOT NULL CHECK (kind IN ('webhook')),
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 100),
    url text NOT NULL CHECK (url ~ '^https?://' AND char_length(url) <= 2000),
    enabled boolean NOT NULL DEFAULT true,
    created_by uuid NOT NULL REFERENCES identities (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, environment_id, id),
    FOREIGN KEY (project_id, environment_id) REFERENCES environments (project_id, id)
);

CREATE TABLE jobs (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    id uuid NOT NULL,
    destination_id uuid NOT NULL,
    -- The release it announces.
    generation bigint NOT NULL,
    -- Sent with every attempt, so a destination can drop a repeat.
    idempotency_key text NOT NULL UNIQUE,
    status text NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'running', 'retrying', 'succeeded', 'failed', 'superseded')),
    attempts integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    lease_owner text CHECK (char_length(lease_owner) <= 200),
    lease_expires_at timestamptz,
    lease_generation integer NOT NULL DEFAULT 0,
    next_attempt_at timestamptz NOT NULL DEFAULT now(),
    deadline_at timestamptz NOT NULL,
    last_http_status smallint,
    last_error text CHECK (char_length(last_error) <= 500),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    finished_at timestamptz,
    CHECK ((status = 'running') = (lease_owner IS NOT NULL AND lease_expires_at IS NOT NULL)),
    PRIMARY KEY (project_id, environment_id, id),
    UNIQUE (project_id, environment_id, destination_id, generation),
    FOREIGN KEY (project_id, environment_id, destination_id)
        REFERENCES destinations (project_id, environment_id, id),
    FOREIGN KEY (project_id, environment_id, generation)
        REFERENCES releases (project_id, environment_id, generation)
);
-- What workers claim: due jobs, and running ones whose lease ran out.
CREATE INDEX jobs_due ON jobs (next_attempt_at) WHERE status IN ('pending', 'retrying');
CREATE INDEX jobs_leased ON jobs (lease_expires_at) WHERE status = 'running';
-- A release's deliveries, for its page.
CREATE INDEX jobs_by_generation ON jobs (project_id, environment_id, generation);

ALTER TABLE destinations ENABLE ROW LEVEL SECURITY;
ALTER TABLE destinations FORCE ROW LEVEL SECURITY;
ALTER TABLE jobs ENABLE ROW LEVEL SECURITY;
ALTER TABLE jobs FORCE ROW LEVEL SECURITY;
CREATE POLICY destinations_in_project ON destinations FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
CREATE POLICY jobs_in_project ON jobs FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
GRANT SELECT, INSERT, UPDATE (name, url, enabled) ON destinations TO colophon_api;
-- The API inserts jobs with a release and supersedes waiting ones; workers do the rest.
GRANT SELECT, INSERT, UPDATE (status, updated_at, finished_at) ON jobs TO colophon_api;

-- The worker works across projects.
CREATE POLICY jobs_for_workers ON jobs FOR ALL TO colophon_worker USING (true) WITH CHECK (true);
CREATE POLICY destinations_for_workers ON destinations FOR SELECT TO colophon_worker USING (true);
CREATE POLICY releases_for_workers ON releases FOR SELECT TO colophon_worker USING (true);
CREATE POLICY publications_for_workers ON publications FOR SELECT TO colophon_worker USING (true);
CREATE POLICY environments_for_workers ON environments FOR SELECT TO colophon_worker USING (true);
GRANT SELECT, UPDATE (status, attempts, lease_owner, lease_expires_at, lease_generation, next_attempt_at,
                      last_http_status, last_error, updated_at, finished_at) ON jobs TO colophon_worker;
GRANT SELECT ON destinations, releases TO colophon_worker;
GRANT SELECT (project_id, environment_id, entry_id, revision_id, collection, title, valid_from) ON publications TO colophon_worker;
GRANT SELECT (project_id, id, slug) ON environments TO colophon_worker;
