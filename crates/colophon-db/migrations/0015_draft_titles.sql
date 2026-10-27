-- Title search reads the draft's title from the entry (step 3.5's search found it through each
-- entry's draft revision: in CI, one search of a 512-entry collection made 512 lookups and took
-- 157 ms, and searches held connections the other requests queued for). docs/modules/02-db.md.
--
-- entries.draft_title copies the draft revision's title; the two statements that move the draft
-- pointer write both. A trigram index serves `ILIKE '%…%'` once collections are large.

-- pg_trgm is a trusted extension: the database's owner may create it without superuser.
CREATE EXTENSION IF NOT EXISTS pg_trgm;

-- The backfill below must see every row; FORCE comes back on at the end (as in 0003).
ALTER TABLE entries NO FORCE ROW LEVEL SECURITY;
ALTER TABLE revisions NO FORCE ROW LEVEL SECURITY;

ALTER TABLE entries ADD COLUMN draft_title text;
UPDATE entries e SET draft_title = r.title
FROM revisions r
WHERE r.project_id = e.project_id AND r.environment_id = e.environment_id AND r.id = e.draft_revision_id;
ALTER TABLE entries ALTER COLUMN draft_title SET NOT NULL;

ALTER TABLE entries FORCE ROW LEVEL SECURITY;
ALTER TABLE revisions FORCE ROW LEVEL SECURITY;

CREATE INDEX entries_draft_title_trgm ON entries USING gin (draft_title gin_trgm_ops);
GRANT UPDATE (draft_title) ON entries TO colophon_api;
