-- D-166: sign-in keys are visible only where their realm signs in.
--
-- sign_in_throttle (0057) holds every realm's keys — normalized emails and client addresses,
-- staff and every project's audience together — with no row-level security, so any query the
-- application ran against it saw them all. Each realm now sees only its own: staff keys outside
-- any project (staff sign-in runs before one exists), a project's audience keys inside that
-- project's context (audience sign-in runs there), and nothing across. Writes are held to the
-- same rule. The retention sweep, a colophon_retention definer function, keeps its view of
-- every row.

ALTER TABLE sign_in_throttle ENABLE ROW LEVEL SECURITY;

CREATE POLICY sign_in_throttle_in_realm ON sign_in_throttle FOR ALL TO colophon_api
    USING (CASE WHEN realm = 'staff'
                THEN nullif(current_setting('app.project_id', true), '') IS NULL
                ELSE realm = 'audience:' || nullif(current_setting('app.project_id', true), '')
           END)
    WITH CHECK (CASE WHEN realm = 'staff'
                     THEN nullif(current_setting('app.project_id', true), '') IS NULL
                     ELSE realm = 'audience:' || nullif(current_setting('app.project_id', true), '')
                END);

CREATE POLICY sign_in_throttle_for_retention ON sign_in_throttle FOR ALL TO colophon_retention
    USING (true);
