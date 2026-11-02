-- Ported from the content-cards line: an entry may name its locale, and entries that are the
-- same document in different locales share a localization group (D-087). docs/modules/02-db.md,
-- 04-api.md.
--
-- `locale` is a BCP-47 tag the API shape-checks — free text, not a table (there is no
-- per-locale fallback or localized-field machinery yet). NULL is the project's default.
-- `localization_id` marks the group; an unlocalized entry has none, and the CHECK keeps a
-- group member's locale from being cleared (the member leaves only by deletion). The partial
-- unique index makes one live entry per locale per group physical, and frees the locale when
-- a sibling is tombstoned — D-080's predicate, like `entries_draft_slug`.
--
-- Localizing is idempotent like creating: `entry.localize` joins the operations a key may
-- claim, so a retry of a lost response replays the recorded answer rather than writing a
-- second sibling (or meeting `entries_one_locale_per_group`).

ALTER TABLE entries ADD COLUMN locale text;
COMMENT ON COLUMN entries.locale IS 'BCP-47 tag the draft is written in, or NULL for the default locale.';
ALTER TABLE entries ADD COLUMN localization_id uuid
    CHECK (localization_id IS NULL OR locale IS NOT NULL);
CREATE UNIQUE INDEX entries_one_locale_per_group
    ON entries (localization_id, locale) WHERE localization_id IS NOT NULL AND deleted_at IS NULL;
CREATE INDEX entries_in_group
    ON entries (localization_id) WHERE localization_id IS NOT NULL;
COMMENT ON COLUMN entries.localization_id IS 'Localizations of the same document share this id; one live entry per locale per group.';

-- The API may label an entry's locale alongside its draft and group siblings it makes; both
-- columns are metadata, granted like `draft_title`, not opened wholesale.
GRANT UPDATE (locale) ON entries TO colophon_api;
GRANT UPDATE (localization_id) ON entries TO colophon_api;

ALTER TABLE idempotency_records DROP CONSTRAINT idempotency_records_operation_check;
ALTER TABLE idempotency_records ADD CONSTRAINT idempotency_records_operation_check
    CHECK (operation IN ('entry.create', 'entry.save', 'entry.delete', 'entry.import',
                         'entry.localize',
                         'content_type.create', 'content_type.update', 'content_type.delete',
                         'changeset.create', 'changeset.publish',
                         'component.create', 'component.update'));
