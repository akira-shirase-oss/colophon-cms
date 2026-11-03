-- Milestone P4, media step 1: asset fields name uploads. docs/modules/07-media.md, D-094.
--
-- A revision's asset fields record what they name the way revision_references record entries
-- (0024, 0025): same path-shaped `field`, same `position`, a foreign key that refuses an id
-- naming no asset of the project and environment. The publish check (releases.rs `bad_asset`)
-- reads the rows a changeset's revisions write; retention (M3) reads them the other way to
-- keep every referenced object.

CREATE TABLE asset_references (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    revision_id uuid NOT NULL,
    -- Where the asset sits: a field id, or a path inside objects, lists and blocks
    -- (`hero/image`, `sections/2/by`, `photos/0`), as 0025 spells it for entries.
    field text NOT NULL CHECK (
        char_length(field) <= 400
        AND field ~ '^[a-z][a-z0-9_]{0,63}(/([0-9]{1,2}|[a-z][a-z0-9_]{0,63})){0,8}$'
    ),
    position smallint NOT NULL CHECK (position BETWEEN 0 AND 99),
    asset_id uuid NOT NULL,
    PRIMARY KEY (project_id, environment_id, revision_id, field, position),
    FOREIGN KEY (project_id, environment_id, revision_id)
        REFERENCES revisions (project_id, environment_id, id),
    -- An upload that is named may not disappear under it (media M3).
    FOREIGN KEY (project_id, environment_id, asset_id)
        REFERENCES assets (project_id, environment_id, id) ON DELETE RESTRICT
);

-- What names an asset: the publish check, and retention's keep-list the other way.
CREATE INDEX asset_references_to ON asset_references (project_id, environment_id, asset_id);

ALTER TABLE asset_references ENABLE ROW LEVEL SECURITY;
ALTER TABLE asset_references FORCE ROW LEVEL SECURITY;
CREATE POLICY asset_references_in_project ON asset_references FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);

-- Written once with their revision, never changed.
GRANT SELECT, INSERT ON asset_references TO colophon_api;
