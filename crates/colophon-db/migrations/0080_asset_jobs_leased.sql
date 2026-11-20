-- Running asset jobs in lease order (the delivery jobs have had `jobs_leased` since 0009): the
-- worker's sweep finds the ones whose lease ran out here and makes them waiting again
-- (assets::release_lapsed), every sweep interval. The claim itself reads only waiting jobs, in
-- order on asset_jobs_due: once it also searched for lapsed leases in one `WHERE … OR …`, which
-- could use no index in order, so every claim read the whole of asset_jobs, every finished job
-- and dead row version included (the assets smoke's 1-3.5 s claims).
CREATE INDEX asset_jobs_leased ON asset_jobs (lease_expires_at)
    WHERE status = 'running';
