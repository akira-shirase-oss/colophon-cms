-- Local-stack database roles (development only; the password is public). Run by
-- scripts/stack.sh after Postgres is up, and safe to run again.
--
--   colophon      the stack's superuser; owns the tables and runs migrations (the migrator)
--   colophon_api  the application's privileges (migration 0001 grants them; no login)
--   colophon_app  the login colophon-server uses: a member of colophon_api, nothing more
--   colophon_delivery  what public delivery may read: published projections only (migration 0008
--                 grants them; no login). colophon_app may switch to it for a transaction
--                 (SET LOCAL ROLE), which drops every other privilege until the transaction ends
--   colophon_worker  what the worker may touch: delivery jobs across every project, their
--                 destinations and the releases they announce (migration 0009; no login), switched
--                 to the same way
DO $$
BEGIN
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'colophon_api') THEN
        CREATE ROLE colophon_api NOLOGIN NOSUPERUSER NOBYPASSRLS;
    END IF;
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'colophon_app') THEN
        CREATE ROLE colophon_app LOGIN PASSWORD 'colophon-dev' NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE IN ROLE colophon_api;
    END IF;
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'colophon_delivery') THEN
        CREATE ROLE colophon_delivery NOLOGIN NOSUPERUSER NOBYPASSRLS;
    END IF;
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'colophon_worker') THEN
        CREATE ROLE colophon_worker NOLOGIN NOSUPERUSER NOBYPASSRLS;
    END IF;
END
$$;
-- Not inherited: colophon_app holds these privileges only after SET ROLE.
GRANT colophon_delivery TO colophon_app WITH INHERIT FALSE, SET TRUE;
GRANT colophon_worker TO colophon_app WITH INHERIT FALSE, SET TRUE;
