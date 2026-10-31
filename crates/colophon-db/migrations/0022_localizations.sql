-- Localizations: entries that are the same document in different locales share a
-- `localization_id` — Strapi's localization-group model. An entry without a group is simply
-- unlocalized. One entry per locale per group is physical: the unique index refuses a second
-- French sibling wherever it is tried. Creating a sibling copies the source's draft and, if
-- the source had no group yet, gives it one — localizing an entry is a two-entry fact.
ALTER TABLE entries ADD COLUMN localization_id uuid
    CHECK (localization_id IS NULL OR locale IS NOT NULL);
CREATE UNIQUE INDEX entries_one_locale_per_group
    ON entries (localization_id, locale) WHERE localization_id IS NOT NULL;
CREATE INDEX entries_in_group
    ON entries (localization_id) WHERE localization_id IS NOT NULL;
COMMENT ON COLUMN entries.localization_id IS 'Localizations of the same document share this id; one entry per locale per group.';
GRANT UPDATE (localization_id) ON entries TO colophon_api;
