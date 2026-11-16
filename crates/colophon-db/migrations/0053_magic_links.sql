-- P7, D-140: audience magic links — passwordless sign-in and signup for a project's visitors.
--
-- A magic link is a capability, like an invitation's token (migration 0005): the row keeps only
-- the token's SHA-256 hash, so the database never holds anything that can sign someone in. A
-- link is single use (`used_at`), short lived (`expires_at`), and superseded: requesting another
-- for the same email retires the previous one, so an old link can't outlive the newest request.
--
-- `return_to` is the site's own landing page, recorded when the link is requested: the CMS
-- emails (or, in development, hands back) `return_to` with the token appended, so the link
-- always lands on the site that asked rather than on the API origin.
--
-- No enumeration: requesting a link answers the same whether or not the email has an account.
-- Verification decides: an existing account signs in, an unknown email is created in the
-- project's open self-assign role when there is one, and anything else is refused.

CREATE TABLE audience_magic_links (
    project_id uuid NOT NULL,
    id uuid NOT NULL,
    email text NOT NULL CHECK (char_length(email) BETWEEN 3 AND 254 AND email = lower(email)),
    token_hash bytea NOT NULL CHECK (length(token_hash) = 32),
    return_to text NOT NULL CHECK (char_length(return_to) BETWEEN 1 AND 2048),
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    used_at timestamptz,
    PRIMARY KEY (project_id, id),
    UNIQUE (project_id, token_hash),
    CHECK (expires_at > created_at),
    FOREIGN KEY (project_id) REFERENCES projects (id)
);
CREATE INDEX audience_magic_links_by_email ON audience_magic_links (project_id, email, created_at DESC);

ALTER TABLE audience_magic_links ENABLE ROW LEVEL SECURITY;
CREATE POLICY audience_magic_links_in_project ON audience_magic_links FOR ALL TO colophon_api
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
GRANT SELECT, INSERT, UPDATE (used_at), DELETE ON audience_magic_links TO colophon_api;
