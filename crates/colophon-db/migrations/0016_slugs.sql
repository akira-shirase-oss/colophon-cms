-- Milestone P3, step 3.6: slugs. docs/modules/01-domain.md ("Slugs and locales"), 02-db.md,
-- D-054.
--
-- A slug field names an entry in URLs, and no two entries of a collection share one: not among
-- drafts, and not among what is published. Each revision records its slug; an entry copies its
-- draft's, and a publication copies its revision's, so both rules are unique indexes the database
-- checks itself. The API's prechecks only name the entry in the way (spec §07: prechecks alone
-- race).
--
-- Until now no content type could have a slug field, so every existing slug is NULL and nothing
-- needs backfilling.

ALTER TABLE revisions ADD COLUMN slug text
    CHECK (char_length(slug) <= 120 AND slug ~ '^[a-z0-9]+(-[a-z0-9]+)*$');

-- Always the draft revision's slug: entries::create and entries::save write both in the
-- statement that moves the draft pointer. A foreign key would need one more unique index on
-- revisions, written by every save, to guard those two statements; a test holds them instead.
ALTER TABLE entries ADD COLUMN draft_slug text;
CREATE UNIQUE INDEX entries_draft_slug ON entries (project_id, environment_id, collection, draft_slug)
    WHERE draft_slug IS NOT NULL;
GRANT UPDATE (draft_slug) ON entries TO colophon_api;

-- Open publications are what readers get at the newest generation, so at most one per slug.
-- Each older generation was the open set when it was committed, so it holds too.
ALTER TABLE publications ADD COLUMN slug text;
CREATE UNIQUE INDEX publications_live_slug ON publications (project_id, environment_id, collection, slug)
    WHERE valid_to IS NULL AND slug IS NOT NULL;
