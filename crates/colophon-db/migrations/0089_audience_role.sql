-- D-235 (slice-1 review): the public audience endpoints run as colophon_audience.
--
-- Their transactions (colophon_db::audience::audience_tx) take the project from a site's public
-- URL, so any visitor opens one for any project. As plain colophon_api with that project set,
-- the data tables' policies would admit the project's entries, revisions, assets and tokens:
-- only review kept those statements to the audience's own tables. colophon_audience makes the
-- database keep them there. audience_tx switches to it first (SET LOCAL ROLE, as delivery and
-- the worker do), which drops every colophon_api privilege until the transaction ends, and it
-- holds only what the audience flows use: below, each grant names the flow that needs it.
-- Anything else (a project's content, its members, its tokens, its environments, another
-- project's anything) is refused with "permission denied", whatever the project context says.
--
-- Session lookups don't come here: colophon_audience_session (0052) is a definer function the
-- server calls before any project context exists, and it is unchanged.

-- The role (as 0087 makes colophon_accounts). Fresh test databases migrate concurrently, so
-- losing a race to create it is fine.
DO $$
BEGIN
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'colophon_audience') THEN
        CREATE ROLE colophon_audience NOLOGIN NOSUPERUSER NOBYPASSRLS;
    END IF;
EXCEPTION WHEN duplicate_object OR unique_violation THEN
    NULL;
END
$$;

-- The server's login may switch to it, never inherit it (as colophon_delivery, colophon_worker).
-- roles.sql and `colophon admin init-roles` grant this too; it is here as well because an
-- upgrade runs migrations, not init-roles, and the new server switches to the role at once.
DO $$
BEGIN
    IF EXISTS (SELECT FROM pg_roles WHERE rolname = 'colophon_app') AND NOT EXISTS (
        SELECT FROM pg_auth_members m
        JOIN pg_roles granted ON granted.oid = m.roleid
        JOIN pg_roles member ON member.oid = m.member
        WHERE granted.rolname = 'colophon_audience' AND member.rolname = 'colophon_app'
          AND m.set_option AND NOT m.inherit_option
    ) THEN
        GRANT colophon_audience TO colophon_app WITH INHERIT FALSE, SET TRUE;
    END IF;
EXCEPTION WHEN unique_violation THEN
    NULL;
END
$$;

-- Every policy below confines the role to the project audience_tx entered.

-- Audience roles. Signup reads which role it may take (audience::sign_up).
CREATE POLICY site_roles_for_audience ON site_roles FOR SELECT TO colophon_audience
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
GRANT SELECT ON site_roles TO colophon_audience;

-- Site users. Signup counts them and inserts one (audience::sign_up); sign-in reads the
-- account and stamps it (audience::sign_in); a spent magic link finds, locks and stamps it
-- (audience::consume_magic_link; SELECT ... FOR UPDATE needs the UPDATE privilege).
CREATE POLICY site_users_for_audience ON site_users FOR ALL TO colophon_audience
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
GRANT SELECT ON site_users TO colophon_audience;
GRANT INSERT (project_id, id, email, password_hash, display_name, role) ON site_users TO colophon_audience;
GRANT UPDATE (last_sign_in_at, failed_sign_ins, locked_until) ON site_users TO colophon_audience;

-- Audience sessions. A new session revokes its user's oldest over the bound, sweeps long-dead
-- rows and inserts itself (audience::create_session); signout revokes one
-- (audience::revoke_session).
CREATE POLICY site_sessions_for_audience ON site_sessions FOR ALL TO colophon_audience
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
GRANT SELECT, DELETE ON site_sessions TO colophon_audience;
GRANT INSERT (project_id, id, site_user_id, token_hash, csrf_hash, expires_at) ON site_sessions TO colophon_audience;
GRANT UPDATE (revoked_at) ON site_sessions TO colophon_audience;

-- Magic links. A request counts the recent ones and inserts one (audience::request_magic_link);
-- spending one finds it and marks it and its siblings used (audience::consume_magic_link).
CREATE POLICY audience_magic_links_for_audience ON audience_magic_links FOR ALL TO colophon_audience
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid)
    WITH CHECK (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
GRANT SELECT ON audience_magic_links TO colophon_audience;
GRANT INSERT (project_id, id, email, token_hash, return_to, expires_at, requested_from)
    ON audience_magic_links TO colophon_audience;
GRANT UPDATE (used_at) ON audience_magic_links TO colophon_audience;

-- Origins. A magic-link request checks its return origin is listed (audience::request_magic_link).
CREATE POLICY audience_origins_for_audience ON audience_origins FOR SELECT TO colophon_audience
    USING (project_id = nullif(current_setting('app.project_id', true), '')::uuid);
GRANT SELECT ON audience_origins TO colophon_audience;

-- The throttle: this project's audience keys only, never staff keys (0062's realm rule without
-- its staff half). Sign-in checks, counts and clears them (audience::sign_in via
-- throttle::paused, count and clear; ON CONFLICT DO UPDATE needs SELECT and UPDATE).
CREATE POLICY sign_in_throttle_for_audience ON sign_in_throttle FOR ALL TO colophon_audience
    USING (realm = 'audience:' || nullif(current_setting('app.project_id', true), ''))
    WITH CHECK (realm = 'audience:' || nullif(current_setting('app.project_id', true), ''));
GRANT SELECT ON sign_in_throttle TO colophon_audience;
GRANT INSERT (realm, email, address, failures, locked_until, trusted) ON sign_in_throttle TO colophon_audience;
GRANT UPDATE (failures, locked_until, trusted, updated_at) ON sign_in_throttle TO colophon_audience;

-- The audit log: append only, as the audience actor (0054's identity), only the visitor auth
-- events, only in the entered project (audience::record_auth). No SELECT: it reads nothing back.
CREATE POLICY audit_events_by_audience ON audit_events FOR INSERT TO colophon_audience
    WITH CHECK (
        project_id = nullif(current_setting('app.project_id', true), '')::uuid
        AND actor_id = 'a1b2c3d4-0000-4000-8000-0000000000a1'::uuid
        AND action IN ('audience.user_signed_up', 'audience.user_signed_in', 'audience.user_signed_out',
                       'audience.magic_link_sent', 'audience.magic_link_spent')
    );
GRANT INSERT (project_id, id, actor_id, action, subject, detail) ON audit_events TO colophon_audience;
-- Its statement trigger appends the change hint for the audit stream (0068's colophon_hint, which
-- runs as the writer): every role that writes a watched table holds INSERT on change_hints.
-- INSERT only; the pump, as the application, reads and deletes them.
GRANT INSERT ON change_hints TO colophon_audience;
