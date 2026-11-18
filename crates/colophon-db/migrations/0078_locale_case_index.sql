-- no-transaction
-- One language per localization group, whatever its case (D-212): built concurrently so writes go on
-- while it builds. If this fails (a duplicate written since 0077 by an older server), the index is
-- left INVALID: drop it (DROP INDEX CONCURRENTLY entries_one_locale_per_group_ci), resolve the
-- duplicate as the runbook says, and migrate again.
CREATE UNIQUE INDEX CONCURRENTLY entries_one_locale_per_group_ci
    ON entries (localization_id, lower(locale)) WHERE localization_id IS NOT NULL AND deleted_at IS NULL;
