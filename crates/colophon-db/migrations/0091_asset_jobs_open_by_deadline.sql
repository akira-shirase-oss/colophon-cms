-- D-237: the asset jobs' deadline sweep reads the jobs that ran out of time, not the table.
--
-- `assets::expire` runs on every worker every 10 seconds and fails the open asset jobs past their
-- deadline. `asset_jobs` had no index for it (`jobs` has `jobs_open_by_deadline`, migration 0009),
-- so each sweep read the whole table: 1,041 blocks at the 23,000 jobs the assets smoke reaches,
-- growing with every upload ever made. This is the same partial index on the open asset jobs.
--
-- A plain CREATE INDEX: every migration runs inside a transaction (the D-234 non-superuser path
-- needs that, and a test pins it), and CREATE INDEX CONCURRENTLY can't. It holds a SHARE lock on
-- asset_jobs while it builds, which lets reads through and makes writes (uploads' jobs, the
-- worker's claims and finishes) wait; the index covers only open jobs, a few thousand at most,
-- while the build reads the whole table once (see D-237 for the measured time at that size).
CREATE INDEX asset_jobs_open_by_deadline ON asset_jobs (deadline_at)
    WHERE status IN ('pending', 'retrying', 'running');
