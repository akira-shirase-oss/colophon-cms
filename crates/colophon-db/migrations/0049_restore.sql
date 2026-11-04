-- Milestone P6: the restore procedure's outbox hold. docs/modules/18-operations.md ("Restore
-- procedure"), docs/modules/05-worker.md, invariant O3, F13, D-126.
--
-- A restore brings back the outbox exactly as the backup held it: jobs 'running' under leases
-- whose workers are gone, jobs 'pending' or 'retrying' that may already have been delivered on
-- the old timeline. O3 says a restore never dispatches outbound work before reconciliation, so
-- the procedure freezes every unfinished job as 'held' — a state no claim, retry or expiry
-- query reads — until `admin restore resume` returns them to 'pending' for a worker to pick
-- up. 'held' is a real state rather than a flag on another state because the truth differs:
-- the job is not queued (nobody may dispatch it) and not running (nobody holds its lease).

ALTER TABLE jobs DROP CONSTRAINT jobs_status_check;
ALTER TABLE jobs ADD CONSTRAINT jobs_status_check
    CHECK (status IN ('pending', 'running', 'retrying', 'held', 'succeeded', 'failed', 'superseded'));
