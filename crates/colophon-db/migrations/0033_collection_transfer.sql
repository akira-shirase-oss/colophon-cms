-- Ported from the content-cards line: a collection exports and imports drafts.
-- docs/modules/02-db.md, 04-api.md, D-086.
--
-- Importing is idempotent, like saving: one upload of at most 512 items lands in one
-- transaction or nothing does, and a retry of a lost response replays the recorded answer
-- rather than importing twice.

ALTER TABLE idempotency_records DROP CONSTRAINT idempotency_records_operation_check;
ALTER TABLE idempotency_records ADD CONSTRAINT idempotency_records_operation_check
    CHECK (operation IN ('entry.create', 'entry.save', 'entry.delete', 'entry.import',
                         'content_type.create', 'content_type.update', 'content_type.delete',
                         'changeset.create', 'changeset.publish',
                         'component.create', 'component.update'));
