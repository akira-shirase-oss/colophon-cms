-- A field declared `unique` must hold a value no other entry in the collection has. What makes
-- it true is this index, not a read that could race: every save of a draft replaces the entry's
-- rows here inside the same transaction, hashing the canonical spelling of each unique field's
-- value, and a second entry offering the same value meets the primary key and is refused.

CREATE TABLE unique_values (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    collection text NOT NULL,
    field_id text NOT NULL,
    entry_id uuid NOT NULL,
    value_hash bytea NOT NULL,
    PRIMARY KEY (project_id, environment_id, collection, field_id, value_hash),
    FOREIGN KEY (project_id, environment_id, entry_id)
        REFERENCES entries (project_id, environment_id, id) ON DELETE CASCADE
);

ALTER TABLE unique_values ENABLE ROW LEVEL SECURITY;
CREATE POLICY unique_values_in_project ON unique_values FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);

GRANT SELECT, INSERT, DELETE ON unique_values TO colophon_api;
