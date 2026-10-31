-- An entry may name the locale its draft is written in. NULL is the project's default;
-- locales are free-text BCP-47 tags the API shape-checks, not a table (bounded: there is
-- no per-locale fallback or localized-field machinery yet).
ALTER TABLE entries ADD COLUMN locale text;
COMMENT ON COLUMN entries.locale IS 'BCP-47 tag the draft is written in, or NULL for the default locale.';

-- The API may label an entry's locale alongside its draft; the tag is metadata, so the
-- column is granted like `draft_title`, not opened wholesale.
GRANT UPDATE (locale) ON entries TO colophon_api;
