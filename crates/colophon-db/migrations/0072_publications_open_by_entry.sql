-- D-185 (R4/R8): what readers have now, by entry, without reading the rows.
--
-- A rollback's preview counts its pins before it reads them — the set as it stood at a release
-- against the set open now — and both the count and the read anti-join the open set by entry.
-- `publications_open_by_revision` holds the open rows by revision, so that join fetched every
-- open row from the heap, and a publication row carries its content: at 48,110 open rows the
-- count read 48,110 wide rows (257 ms, a sequential scan). This index holds the open rows by
-- entry with the revision and collection beside them, so the open set is an index-only scan
-- (40 ms at the same size, measured with EXPLAIN ANALYZE as the API's role). It is partial, like
-- the open-by-revision one: it grows with what is live, never with history.
CREATE INDEX publications_open_by_entry
    ON publications (project_id, environment_id, entry_id)
    INCLUDE (revision_id, collection, valid_from, revoked_in)
    WHERE valid_to IS NULL;
