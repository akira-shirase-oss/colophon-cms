-- no-transaction
-- The case-sensitive index (0034) is covered by 0078's case-insensitive one (D-212).
DROP INDEX CONCURRENTLY IF EXISTS entries_one_locale_per_group;
