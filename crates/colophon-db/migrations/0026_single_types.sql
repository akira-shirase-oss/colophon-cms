-- Milestone M, step M.4: single types. docs/modules/01-domain.md, 02-db.md, D-067.
--
-- A content type marked `"single": true` holds at most one entry in an environment: a site's
-- settings, its home page. Each entry copies the flag from its content type's current version
-- when it is made (entries::create reads it inside the INSERT, under the content type's shared
-- lock), and a type that becomes single, or stops being one, flips the flag on its entries
-- under the exclusive lock (content_types::set_single). The index below is the rule: a second
-- entry of a single type can't commit, however two creations race.
ALTER TABLE entries ADD COLUMN single boolean NOT NULL DEFAULT false;
CREATE UNIQUE INDEX entries_single ON entries (project_id, environment_id, collection) WHERE single;
GRANT UPDATE (single) ON entries TO colophon_api;
