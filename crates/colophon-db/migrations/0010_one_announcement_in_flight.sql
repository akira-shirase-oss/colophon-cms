-- Milestone P2, step 2.4: one announcement in flight per destination. docs/modules/05-worker.md,
-- D-047.
--
-- Designing the simulator's model of the worker (step 2.4) showed a hole left by step 2.3: while
-- one worker was still sending release N to a destination, release N+1 could commit and another
-- worker could claim and send it, and a slow network could deliver N after N+1. Workers now skip
-- a job whose destination already has a running job with a live lease; leases outlast an
-- attempt's deadline, so once a lease runs out no request of its holder is still on its way.
-- This index keeps that check cheap however many jobs a destination has had.

CREATE INDEX jobs_running_by_destination ON jobs (project_id, environment_id, destination_id)
    WHERE status = 'running';
