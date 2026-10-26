-- Milestone P3, step 3.3: a receipt for every publish. docs/modules/06-delivery.md ("Evidence
-- probes"), D-051.
--
-- A destination can be a probe: a URL whose answer names the generation it serves (the
-- Colophon-Generation header, or a colophon-generation marker in a page). Its job for a release
-- passes once the probe sees that release or a later one. A probe may be pinned to a location:
-- only a worker that runs there (COLOPHON_WORKER_LOCATION) claims its jobs, so "verified at 2
-- locations" means two probes, each seen from its own place.
--
-- Every probe attempt leaves an observation: the release it looked for, where from, the
-- generation it saw, the HTTP status, a hash of the body, and any error. Observations are
-- evidence for receipts, never edited; the worker writes them with the attempt's result, through
-- its lease.

ALTER TABLE destinations DROP CONSTRAINT destinations_kind_check;
ALTER TABLE destinations ADD CONSTRAINT destinations_kind_check CHECK (kind IN ('webhook', 'cdn', 'probe'));
ALTER TABLE destinations ADD COLUMN location text CHECK (location ~ '^[a-z0-9][a-z0-9-]{0,62}$');
GRANT SELECT (location) ON destinations TO colophon_worker;

CREATE TABLE observations (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    id uuid NOT NULL,
    job_id uuid NOT NULL,
    destination_id uuid NOT NULL,
    -- The release the probe was looking for.
    generation bigint NOT NULL,
    -- Where it looked from: the worker's location label.
    location text NOT NULL CHECK (char_length(location) BETWEEN 1 AND 63),
    -- What it saw, if the answer named a generation.
    observed_generation bigint,
    http_status smallint,
    body_hash bytea CHECK (length(body_hash) = 32),
    error text CHECK (char_length(error) <= 500),
    observed_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, environment_id, id),
    FOREIGN KEY (project_id, environment_id, job_id) REFERENCES jobs (project_id, environment_id, id),
    FOREIGN KEY (project_id, environment_id, destination_id)
        REFERENCES destinations (project_id, environment_id, id)
);
-- A receipt's evidence: what was seen at or after a release, and the attempts made for it.
CREATE INDEX observations_by_generation ON observations (project_id, environment_id, observed_generation);
CREATE INDEX observations_by_release ON observations (project_id, environment_id, generation);
CREATE INDEX observations_by_job ON observations (project_id, environment_id, job_id);

-- A destination's latest open job, for receipts: open jobs are few, so this stays small, and the
-- latest one is found without reading the destination's history.
CREATE INDEX jobs_open_by_destination ON jobs (project_id, environment_id, destination_id, generation)
    WHERE status IN ('pending', 'retrying', 'running');

ALTER TABLE observations ENABLE ROW LEVEL SECURITY;
ALTER TABLE observations FORCE ROW LEVEL SECURITY;
CREATE POLICY observations_in_project ON observations FOR SELECT TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
GRANT SELECT ON observations TO colophon_api;
CREATE POLICY observations_for_workers ON observations FOR ALL TO colophon_worker USING (true) WITH CHECK (true);
GRANT SELECT, INSERT ON observations TO colophon_worker;
