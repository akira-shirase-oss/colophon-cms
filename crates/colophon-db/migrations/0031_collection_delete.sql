-- Ported from the content-cards line: deleting a collection. docs/modules/02-db.md,
-- 04-api.md, D-083.
--
-- A collection and every version of its definition are deleted outright — unlike an entry,
-- a collection leaves no tombstone; the slug is free to take again. It is possible only while
-- nothing of it remains in the content: the application's role refuses while it sees live
-- entries, and the foreign keys refuse while deleted entries' revisions still pin its versions
-- (revisions are insert-only and tombstones never move, so a collection that has ever held an
-- entry keeps its history and stays).
--
-- The API role gets DELETE on both tables; the in_project row policies are FOR ALL, so the
-- same project scoping applies.

GRANT DELETE ON collections TO colophon_api;
GRANT DELETE ON content_type_versions TO colophon_api;

-- Deleting is idempotent, like saving.
ALTER TABLE idempotency_records DROP CONSTRAINT idempotency_records_operation_check;
ALTER TABLE idempotency_records ADD CONSTRAINT idempotency_records_operation_check
    CHECK (operation IN ('entry.create', 'entry.save', 'entry.delete', 'content_type.create',
                         'content_type.update', 'content_type.delete', 'changeset.create',
                         'changeset.publish', 'component.create', 'component.update'));
