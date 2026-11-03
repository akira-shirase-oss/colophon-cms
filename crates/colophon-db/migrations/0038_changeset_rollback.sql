-- A changeset built by rolling back to a release (D-097). rollback_generation names the release
-- whose publication set it restores; NULL is an ordinary changeset. Its publish pins name older
-- revisions deliberately, so "the draft moved since pinning" does not apply to them — a pinned
-- revision cannot drift; the column is what the staleness queries read to know that.

ALTER TABLE changesets ADD COLUMN rollback_generation bigint;
