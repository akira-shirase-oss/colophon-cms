-- An undo of a release (D-184): the changeset that put back exactly what release `undo_of` took
-- down, published by the person who made that takedown while it was still the newest release.
-- It rides on the rollback mechanism (D-097) — its publish pins name the revisions readers had,
-- so `rollback_generation` is set to the release before the takedown, which is the publication
-- set the undo restores — and `undo_of` is what tells the two apart, so a receipt, a release list
-- and the Activity log can say "Undo of release N" instead of "restores release N-1". NULL for
-- every ordinary and rollback changeset. The CHECK keeps the pair honest: an undo always restores
-- the release just before the one it undoes. `rollback_generation IS NOT NULL` is spelled out
-- because a comparison with NULL is NULL, which a CHECK lets through.

ALTER TABLE changesets ADD COLUMN undo_of bigint;

ALTER TABLE changesets ADD CONSTRAINT changesets_undo_of_restores_previous
    CHECK (undo_of IS NULL
           OR (undo_of >= 1 AND rollback_generation IS NOT NULL AND rollback_generation = undo_of - 1));
