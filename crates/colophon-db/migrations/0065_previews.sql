-- Gap 1, D-171: editor draft preview on the editor's own site. docs/modules/03-auth.md
-- ("Preview grants"), 06-delivery.md ("Preview reads").
--
-- A preview is a delegated, scoped, short-lived read. A person who may read the entry issues a
-- grant for one scope — an entry (with its translations), or a publish request bound to its
-- hash — and gets a one-time code (60 s). The customer's site exchanges the code for a bearer
-- token (15 minutes by default, at most 60; renewable while the Studio holds it, never past
-- 8 hours from creation). Both are 256-bit random strings stored as SHA-256 hashes: neither the
-- code nor the token is ever stored. The exchange clears `code_hash` in the same UPDATE that
-- sets `token_hash`, so two racing exchanges can't both succeed.
--
-- Every read with the token re-checks the grant here and the issuer's membership and role in
-- `memberships` — the grant names the issuer, never a copy of their permissions — so removing
-- the member or narrowing the role ends the preview on the next request.
--
-- Where the site's preview route lives: one address template per environment, which must hold
-- `{code}`. The API validates the rest (https only, except plain http on localhost for a
-- development server; the placeholders it knows); the CHECK here keeps the essentials physical.
--
-- Both tables are project-scoped by row-level security like every project table, and both
-- cascade from their environment, so deleting a project (colophon_delete_project, which deletes
-- environments) takes them with it. Entries and publish requests are named by id only: a grant
-- whose entry is deleted or whose request is published simply stops answering.

CREATE TABLE preview_targets (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    url_template text NOT NULL CHECK (
        char_length(url_template) BETWEEN 15 AND 2048
        AND url_template ~ '^https?://'
        AND strpos(url_template, '{code}') > 0
    ),
    updated_by uuid NOT NULL REFERENCES identities (id),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, environment_id),
    FOREIGN KEY (project_id, environment_id)
        REFERENCES environments (project_id, id) ON DELETE CASCADE
);

CREATE TABLE preview_grants (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    id uuid NOT NULL,
    issuer_id uuid NOT NULL REFERENCES identities (id),
    scope_kind text NOT NULL CHECK (scope_kind IN ('entry', 'changeset')),
    entry_id uuid,
    changeset_id uuid,
    changeset_hash bytea CHECK (changeset_hash IS NULL OR length(changeset_hash) = 32),
    ttl_seconds integer NOT NULL CHECK (ttl_seconds BETWEEN 60 AND 3600),
    code_hash bytea UNIQUE CHECK (code_hash IS NULL OR length(code_hash) = 32),
    code_expires_at timestamptz,
    token_hash bytea UNIQUE CHECK (token_hash IS NULL OR length(token_hash) = 32),
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    exchanged_at timestamptz,
    revoked_at timestamptz,
    PRIMARY KEY (project_id, environment_id, id),
    CHECK ((scope_kind = 'entry')
           = (entry_id IS NOT NULL AND changeset_id IS NULL AND changeset_hash IS NULL)),
    CHECK ((scope_kind = 'changeset')
           = (changeset_id IS NOT NULL AND changeset_hash IS NOT NULL AND entry_id IS NULL)),
    CHECK (code_hash IS NULL OR code_expires_at IS NOT NULL),
    CHECK (expires_at > created_at AND expires_at <= created_at + interval '8 hours'),
    FOREIGN KEY (project_id, environment_id)
        REFERENCES environments (project_id, id) ON DELETE CASCADE
);
-- "My live previews of this entry" (the editor's sidebar) and the per-project live bound.
CREATE INDEX preview_grants_live ON preview_grants (project_id, expires_at)
    WHERE revoked_at IS NULL;
CREATE INDEX preview_grants_by_issuer ON preview_grants (project_id, issuer_id, entry_id)
    WHERE revoked_at IS NULL;

ALTER TABLE preview_targets ENABLE ROW LEVEL SECURITY;
ALTER TABLE preview_targets FORCE ROW LEVEL SECURITY;
ALTER TABLE preview_grants ENABLE ROW LEVEL SECURITY;
ALTER TABLE preview_grants FORCE ROW LEVEL SECURITY;
CREATE POLICY preview_targets_in_project ON preview_targets FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
CREATE POLICY preview_grants_in_project ON preview_grants FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
GRANT SELECT, INSERT, UPDATE, DELETE ON preview_targets TO colophon_api;
-- Grants are never deleted by the application (revocation is a timestamp); the retention
-- sweep removes them a week after they end.
GRANT SELECT, INSERT ON preview_grants TO colophon_api;
GRANT UPDATE (code_hash, code_expires_at, token_hash, expires_at, exchanged_at, revoked_at)
    ON preview_grants TO colophon_api;

-- Ended grants go a week after they end (D-165's pattern): a bounded, oldest-first delete
-- owned by colophon_retention, which the worker may execute but never see past.
CREATE POLICY preview_grants_for_retention ON preview_grants FOR ALL TO colophon_retention
    USING (true) WITH CHECK (true);
GRANT SELECT, DELETE ON preview_grants TO colophon_retention;
CREATE FUNCTION colophon_sweep_preview_grants(cutoff timestamptz, batch bigint)
    RETURNS bigint LANGUAGE plpgsql VOLATILE SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE removed bigint;
BEGIN
    WITH doomed AS (
        SELECT project_id, environment_id, id FROM public.preview_grants
        WHERE least(expires_at, coalesce(revoked_at, expires_at)) < cutoff
        ORDER BY expires_at LIMIT batch)
    DELETE FROM public.preview_grants g USING doomed d
    WHERE g.project_id = d.project_id AND g.environment_id = d.environment_id AND g.id = d.id;
    GET DIAGNOSTICS removed = ROW_COUNT;
    RETURN removed;
END $$;
ALTER FUNCTION colophon_sweep_preview_grants(timestamptz, bigint) OWNER TO colophon_retention;
REVOKE ALL ON FUNCTION colophon_sweep_preview_grants(timestamptz, bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_sweep_preview_grants(timestamptz, bigint) TO colophon_worker;
