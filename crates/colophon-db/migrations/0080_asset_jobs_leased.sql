-- The asset claim's second half: running jobs whose lease ran out, in lease order (the delivery
-- jobs have had `jobs_leased` since 0009). Without it the claim's "or a lapsed lease" branch
-- could use no index, so every claim read the whole of asset_jobs, every finished job and every
-- dead row version included, and sorted the due set to take one to four: a cost that grew with
-- everything the table ever held (the assets smoke's 1-3.5 s claims). With it, each half of the
-- claim is an ordered index scan that stops at its limit.
CREATE INDEX asset_jobs_leased ON asset_jobs (lease_expires_at)
    WHERE status = 'running';
