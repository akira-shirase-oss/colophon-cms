-- Milestone P3, step 3.2, part 2: purges. docs/modules/06-delivery.md ("CDN headers and purge"),
-- D-050.
--
-- A destination can be a CDN: its job for a release is a purge by tag rather than an
-- announcement. A purge covers every release since the destination's last purge that
-- succeeded, so one skipped because a newer release was on its way (superseded) is never lost:
-- the newer job purges what both changed. The worker builds the tags from what those releases
-- opened, closed and took down, so it reads when publications ended and the tombstones too.

ALTER TABLE destinations DROP CONSTRAINT destinations_kind_check;
ALTER TABLE destinations ADD CONSTRAINT destinations_kind_check CHECK (kind IN ('webhook', 'cdn'));

GRANT SELECT (valid_to) ON publications TO colophon_worker;
CREATE POLICY tombstones_for_workers ON tombstones FOR SELECT TO colophon_worker USING (true);
GRANT SELECT ON tombstones TO colophon_worker;

-- What a run of releases closed (replaced or took down), without reading every publication.
CREATE INDEX publications_closed_by_release ON publications (project_id, environment_id, valid_to)
    WHERE valid_to IS NOT NULL;
