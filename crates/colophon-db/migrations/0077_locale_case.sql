-- Locale tags are case-insensitive (BCP 47, RFC 5646 §2.1.1), but `entries_one_locale_per_group`
-- (0034) compared them as text, so one localization group could hold both `fr` and `FR` and a
-- reader asking for French got either (D-212). This migration stores every tag in the one case
-- BCP 47 recommends, the same rule as `colophon_domain::locale::canonical`, and keeps it so: a
-- trigger recases every tag written, whoever writes it (an older server during a deploy, an
-- import, psql, a future code path). Case variants recase to one string, so 0034's unique index on
-- `(localization_id, locale)` then refuses `FR` beside `fr` as it is, with no new index.
--
-- A group that already holds two tags differing only in case is not merged or deleted here: that
-- would choose between two people's entries. The migration refuses, naming each group and its
-- tags, and the runbook (deploy/runbooks/upgrade.md, "Locales that differ only in case") says how
-- to keep one: delete the other through the API, then run the migration again.

SET LOCAL lock_timeout = '5s';

CREATE FUNCTION colophon_canonical_locale(tag text) RETURNS text
    LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE
    SET search_path = pg_catalog
AS $$
DECLARE
    parts text[] := string_to_array(tag, '-');
    out text[] := '{}';
    sub text;
    after_singleton boolean := false;
BEGIN
    FOR i IN 1 .. coalesce(array_length(parts, 1), 0) LOOP
        sub := parts[i];
        IF i = 1 OR after_singleton THEN
            out := out || lower(sub);
        ELSIF length(sub) = 1 THEN
            after_singleton := true;
            out := out || lower(sub);
        ELSIF length(sub) = 4 AND sub ~ '^[A-Za-z]{4}$' THEN
            out := out || (upper(substr(sub, 1, 1)) || lower(substr(sub, 2)));
        ELSIF length(sub) = 2 AND sub ~ '^[A-Za-z]{2}$' THEN
            out := out || upper(sub);
        ELSE
            out := out || lower(sub);
        END IF;
    END LOOP;
    RETURN array_to_string(out, '-');
END
$$;
COMMENT ON FUNCTION colophon_canonical_locale(text) IS
    'A BCP 47 tag in its recommended case (D-212): language lowercase, script titlecase, region uppercase, the rest lowercase. Mirrors colophon_domain::locale::canonical.';

DO $$
DECLARE
    clashes text;
BEGIN
    SELECT string_agg(format('group %s: %s', localization_id, tags), '; ' ORDER BY localization_id)
      INTO clashes
      FROM (SELECT localization_id, string_agg(locale, ', ' ORDER BY locale) AS tags
              FROM entries
             WHERE localization_id IS NOT NULL AND deleted_at IS NULL
             GROUP BY localization_id, lower(locale)
            HAVING count(*) > 1) AS twice;
    IF clashes IS NOT NULL THEN
        RAISE EXCEPTION 'locale_case_duplicates: these localization groups hold one language twice, in different case: %', clashes
            USING HINT = 'Keep one entry of each and delete the other through the API, then migrate again (deploy/runbooks/upgrade.md, "Locales that differ only in case").';
    END IF;
END
$$;

UPDATE entries SET locale = colophon_canonical_locale(locale)
 WHERE locale IS NOT NULL AND locale <> colophon_canonical_locale(locale);

-- From here on every tag written is recased on its way in, so stored tags stay canonical.
CREATE FUNCTION colophon_entries_canonical_locale() RETURNS trigger
    LANGUAGE plpgsql
    SET search_path = pg_catalog, public
AS $$
BEGIN
    NEW.locale := public.colophon_canonical_locale(NEW.locale);
    RETURN NEW;
END
$$;
CREATE TRIGGER entries_canonical_locale
    BEFORE INSERT OR UPDATE OF locale ON entries
    FOR EACH ROW WHEN (NEW.locale IS NOT NULL)
    EXECUTE FUNCTION colophon_entries_canonical_locale();
