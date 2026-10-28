-- Milestone M, step M.4: unique fields. docs/modules/01-domain.md, 02-db.md, D-070.
--
-- A field marked `"unique": true` holds a value no other entry of its collection holds: not
-- among drafts, and not among what is published, as with slugs (D-054). Each entry claims the
-- values of its draft and of its open publication here, one row per unique field it sets, and
-- the constraint `unique_claims_value` is the rule: a second claim on a value can't commit,
-- however two writes race. A value is compared by the SHA-256 of its JSON text (canonical
-- content has one form per value), so a long text still fits an index entry.
--
-- Claims exist only for the fields the content type's current version marks unique, and are
-- kept by the statements that move what they describe: entries::create and entries::save
-- replace an entry's draft claims as they move its draft, releases::commit replaces the
-- published claims of every entry a release touches, and a content type change
-- (content_types::claim_unique, under the type's exclusive lock) claims the values of fields
-- it makes unique and drops the claims of fields that stop being unique.

CREATE TABLE unique_claims (
    project_id uuid NOT NULL,
    environment_id uuid NOT NULL,
    collection text NOT NULL,
    field text NOT NULL CHECK (field ~ '^[a-z][a-z0-9_]{0,63}$'),
    published boolean NOT NULL,
    value_hash bytea NOT NULL CHECK (length(value_hash) = 32),
    entry_id uuid NOT NULL,
    PRIMARY KEY (project_id, environment_id, entry_id, published, field),
    CONSTRAINT unique_claims_value
        UNIQUE (project_id, environment_id, collection, field, published, value_hash),
    FOREIGN KEY (project_id, environment_id, entry_id, collection)
        REFERENCES entries (project_id, environment_id, id, collection)
);

ALTER TABLE unique_claims ENABLE ROW LEVEL SECURITY;
ALTER TABLE unique_claims FORCE ROW LEVEL SECURITY;
CREATE POLICY unique_claims_in_project ON unique_claims FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);

GRANT SELECT, INSERT, DELETE ON unique_claims TO colophon_api;

-- The ids of a canonical definition's unique fields. `"unique":true` is written only when set,
-- and can't occur inside a string (its quotes would be escaped), so a definition without that
-- text has none and isn't parsed.
CREATE FUNCTION colophon_unique_fields(definition text) RETURNS text[]
    LANGUAGE sql
    IMMUTABLE STRICT PARALLEL SAFE
    SET search_path = pg_catalog, pg_temp
    RETURN CASE
        WHEN strpos(definition, '"unique":true') = 0 THEN ARRAY[]::text[]
        ELSE ARRAY(
            SELECT found.id #>> '{}'
            FROM jsonb_path_query(definition::jsonb, '$.fields[*] ? (@.unique == true).id')
                AS found (id)
        )
    END;

-- One row for each of `fields` that `content` sets: the field and the SHA-256 of its value's
-- JSON text. PL/pgSQL, so the content is parsed once, and not at all when `fields` is empty (a
-- SQL function's planner may fold the parse in as a constant).
CREATE FUNCTION colophon_unique_values(fields text[], content text)
    RETURNS TABLE (field text, value_hash bytea)
    LANGUAGE plpgsql
    IMMUTABLE STRICT PARALLEL SAFE
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    body jsonb;
BEGIN
    IF cardinality(fields) = 0 THEN
        RETURN;
    END IF;
    body := content::jsonb;
    RETURN QUERY
        SELECT wanted.id, sha256(convert_to((body -> wanted.id)::text, 'UTF8'))
        FROM unnest(fields) AS wanted (id)
        WHERE body -> wanted.id IS NOT NULL AND body -> wanted.id <> 'null'::jsonb;
END;
$$;

REVOKE ALL ON FUNCTION colophon_unique_fields(text) FROM PUBLIC;
REVOKE ALL ON FUNCTION colophon_unique_values(text[], text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_unique_fields(text) TO colophon_api;
GRANT EXECUTE ON FUNCTION colophon_unique_values(text[], text) TO colophon_api;
