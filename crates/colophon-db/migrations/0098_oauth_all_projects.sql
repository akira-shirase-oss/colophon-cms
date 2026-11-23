-- A sign-in connection acts in all of its person's projects (D-248): an OAuth grant with no
-- project acts as the person in every project they are a member of, now or later, with their role
-- in each, decided per request by the API exactly as for their own session. A grant with a
-- project (D-246's) keeps acting in that one project alone.
ALTER TABLE oauth_grants ALTER COLUMN project_id DROP NOT NULL;
COMMENT ON COLUMN oauth_grants.project_id IS
    'The one project the grant acts in; NULL: every project its identity is a member of (D-248).';
