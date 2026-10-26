-- Milestone P1, step 1.8: the first owner. docs/modules/03-auth.md ("Owner bootstrap"), D-043.
--
-- An installation's administrators are the people who may create projects. The first one is made
-- by `colophon admin bootstrap`, run by the operator as the migrator, which binds it to an
-- identity provider's issuer and subject, never to an email address. The bootstrap is recorded
-- once and can never run again: the installation row can't be changed or removed, even by the
-- table owner, without dropping the triggers below on purpose.
--
-- The application role can read both tables and write neither: no request, however it is
-- authenticated, can make someone an administrator.

CREATE TABLE installation (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    bootstrapped_by uuid NOT NULL REFERENCES identities (id),
    bootstrapped_at timestamptz NOT NULL DEFAULT now()
);

CREATE FUNCTION installation_is_permanent() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'the installation record is permanent: bootstrap runs once'
        USING ERRCODE = 'insufficient_privilege';
END;
$$;
CREATE TRIGGER installation_is_permanent
    BEFORE UPDATE OR DELETE ON installation
    FOR EACH ROW EXECUTE FUNCTION installation_is_permanent();
CREATE TRIGGER installation_is_never_truncated
    BEFORE TRUNCATE ON installation
    FOR EACH STATEMENT EXECUTE FUNCTION installation_is_permanent();

CREATE TABLE administrators (
    identity_id uuid PRIMARY KEY REFERENCES identities (id),
    granted_at timestamptz NOT NULL DEFAULT now(),
    -- Who made them an administrator; NULL for the one the bootstrap made.
    granted_by uuid REFERENCES identities (id)
);

GRANT SELECT ON installation, administrators TO colophon_api;

-- Below the API: a project can only be created by an administrator.
CREATE POLICY projects_created_by_administrators ON projects AS RESTRICTIVE FOR INSERT TO colophon_api
    WITH CHECK (EXISTS (
        SELECT 1 FROM administrators a
        WHERE a.identity_id = nullif(current_setting('app.identity_id', true), '')::uuid
    ));
