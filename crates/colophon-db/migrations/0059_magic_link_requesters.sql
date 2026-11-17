-- D-161: who asked for a magic link, so one client can't spend a project's link budget.
--
-- D-153's per-project ceiling (a hundred links a minute) bounded what unauthenticated requests
-- could make a project's mail provider send, but it was one bucket for everyone: a single
-- client asking twice a second kept every other visitor at 429 — and a visitor whose account
-- a link created has no usable password, so that locked them out of the site entirely. Each
-- link now records the client address it was asked from (IPv6 by its /64, as admission keys
-- it), and a request first counts that address's links in the project: an address over its own
-- share is refused before it can touch the shared ceiling. Older rows have no address and
-- count against nothing.

ALTER TABLE audience_magic_links
    ADD COLUMN requested_from text CHECK (char_length(requested_from) BETWEEN 1 AND 64);

CREATE INDEX audience_magic_links_by_requester
    ON audience_magic_links (project_id, requested_from, created_at)
    WHERE requested_from IS NOT NULL;
