-- D-148: the origins a project's magic links may land on.
--
-- A magic link is a credential: whoever receives the URL signs in as the address it was sent
-- to. Until now `return_to` was any absolute http(s) URL the requester chose, and the token was
-- appended to it — so anyone could ask for a link to a victim's address with their own site as
-- `return_to`, and the victim's click delivered the token to them. A link now lands only on an
-- origin (`scheme://host[:port]`, exactly as browsers serialize it) that the project's staff
-- listed here; a project that lists none sends no links at all, which is the safe default.
--
-- The list is small and staff-managed (`audience.manage`, replaced as a whole by
-- `PUT {P}/audience/origins`), read once per link request inside the request's own
-- project-scoped transaction. Row-level security confines it to the project like every
-- audience table.

CREATE TABLE audience_origins (
    project_id uuid NOT NULL REFERENCES projects (id),
    -- Lowercase scheme://host[:port], no path: what `url::Url::origin().ascii_serialization()`
    -- answers for an http(s) URL. Brackets and colons cover IPv6 literals.
    origin text NOT NULL CHECK (
        char_length(origin) BETWEEN 8 AND 255
        AND origin ~ '^https?://[a-z0-9.\[\]:-]+$'
    ),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, origin)
);

ALTER TABLE audience_origins ENABLE ROW LEVEL SECURITY;
ALTER TABLE audience_origins FORCE ROW LEVEL SECURITY;
CREATE POLICY audience_origins_in_project ON audience_origins FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
GRANT SELECT, INSERT, DELETE ON audience_origins TO colophon_api;
