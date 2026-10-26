-- Milestone P2, step 2.2: one release, one number. docs/modules/01-domain.md ("Releases"), D-045.
--
-- Publishing an approved changeset commits a release: the environment's generation goes up by
-- exactly one, but only if it is still the generation the publisher saw (a conditional update),
-- so two releases prepared on one base can't both commit. In the same transaction each pinned
-- revision is copied into `publications`, the published projection: one row per stretch of an
-- entry's publication history, a half-open interval of generations [valid_from, valid_to) that
-- an exclusion constraint keeps from overlapping.
--
-- Public delivery reads only `publications`, `releases` and environments' generations, as
-- colophon_delivery: a role with no other privileges, which the application switches to for
-- each delivery transaction. Drafts are out of its reach by permission, not by convention.
-- The role itself comes from deploy/postgres/roles.sql, as colophon_api does.

CREATE EXTENSION IF NOT EXISTS btree_gist;

GRANT UPDATE (generation) ON environments TO colophon_api;

CREATE TABLE releases (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    generation bigint NOT NULL CHECK (generation >= 1),
    epoch bigint NOT NULL,
    changeset_id uuid NOT NULL,
    changeset_hash bytea NOT NULL CHECK (length(changeset_hash) = 32),
    -- The approval it was published under.
    review_id uuid NOT NULL,
    published_by uuid NOT NULL REFERENCES identities (id),
    published_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, environment_id, generation),
    -- A changeset is published once.
    UNIQUE (project_id, environment_id, changeset_id),
    FOREIGN KEY (project_id, environment_id) REFERENCES environments (project_id, id),
    FOREIGN KEY (project_id, environment_id, changeset_id)
        REFERENCES changesets (project_id, environment_id, id),
    FOREIGN KEY (project_id, environment_id, review_id)
        REFERENCES reviews (project_id, environment_id, id)
);

CREATE TABLE publications (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    entry_id uuid NOT NULL,
    revision_id uuid NOT NULL,
    collection text NOT NULL,
    title text NOT NULL,
    -- A copy of the revision's canonical JSON: delivery never reads the revisions table.
    content text NOT NULL CHECK (octet_length(content) <= 263168),
    content_hash bytea NOT NULL CHECK (length(content_hash) = 32),
    valid_from bigint NOT NULL,
    valid_to bigint CHECK (valid_to > valid_from),
    PRIMARY KEY (project_id, environment_id, entry_id, valid_from),
    FOREIGN KEY (project_id, environment_id, valid_from)
        REFERENCES releases (project_id, environment_id, generation),
    FOREIGN KEY (project_id, environment_id, valid_to)
        REFERENCES releases (project_id, environment_id, generation),
    FOREIGN KEY (project_id, environment_id, entry_id, revision_id)
        REFERENCES revisions (project_id, environment_id, entry_id, id),
    EXCLUDE USING gist (
        project_id WITH =, environment_id WITH =, entry_id WITH =,
        int8range(valid_from, valid_to) WITH &&
    )
);
-- Delivery: a collection's entries at a generation, newest first.
CREATE INDEX publications_by_collection ON publications (project_id, environment_id, collection, entry_id DESC);

ALTER TABLE releases ENABLE ROW LEVEL SECURITY;
ALTER TABLE releases FORCE ROW LEVEL SECURITY;
ALTER TABLE publications ENABLE ROW LEVEL SECURITY;
ALTER TABLE publications FORCE ROW LEVEL SECURITY;
CREATE POLICY releases_in_project ON releases FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
CREATE POLICY publications_in_project ON publications FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
GRANT SELECT, INSERT ON releases TO colophon_api;
GRANT SELECT, INSERT, UPDATE (valid_to) ON publications TO colophon_api;

-- Delivery: read-only, inside the requested project's context.
CREATE POLICY releases_delivered ON releases FOR SELECT TO colophon_delivery
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
CREATE POLICY publications_delivered ON publications FOR SELECT TO colophon_delivery
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
CREATE POLICY environments_delivered ON environments FOR SELECT TO colophon_delivery
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
GRANT SELECT ON releases, publications TO colophon_delivery;
GRANT SELECT (project_id, id, slug, epoch, generation) ON environments TO colophon_delivery;

-- Publishing is idempotent too: a retry after a lost answer gets the same release.
ALTER TABLE idempotency_records DROP CONSTRAINT idempotency_records_operation_check;
ALTER TABLE idempotency_records ADD CONSTRAINT idempotency_records_operation_check
    CHECK (operation IN ('entry.create', 'entry.save', 'content_type.create', 'content_type.update',
                         'changeset.create', 'changeset.publish'));
