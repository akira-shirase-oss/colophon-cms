-- Milestone M follow-up: two lookups by revision. docs/modules/02-db.md.
--
-- entries::referrers (who names an entry) joins revision_references to the entry whose draft is
-- a revision, and to the open publication of a revision; releases::commit's reference check
-- joins publications by revision the same way. Neither column was indexed, so each reference
-- scanned every entry, or every publication, of the environment. Found under load: one read of a
-- person named by 235 references, in an environment of 591 entries, discarded 138,877 rows and
-- took 108 ms, and it grows with the environment.
--
-- A draft's update already changes indexed columns (draft_slug, draft_title), so the first index
-- costs saves no heap-only updates they had.
CREATE INDEX entries_by_draft_revision ON entries (project_id, environment_id, draft_revision_id);
CREATE INDEX publications_open_by_revision ON publications (project_id, environment_id, revision_id)
    WHERE valid_to IS NULL;
