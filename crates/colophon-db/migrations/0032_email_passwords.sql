-- Ported from the content-cards line: email and password as a sign-in issuer beside OIDC.
-- docs/modules/03-auth.md, D-084.
--
-- A password account is an ordinary identity whose issuer is `password` and whose subject is a
-- normalized email, so one (issuer, subject) pair still names one account — the existing
-- UNIQUE (issuer, subject) is what refuses a second account for the same email. The extra
-- columns hold the credential and the lockout state; `email` is indexed for a second way to
-- look an account up and constrained to the normalized form the API writes.
--
-- `password_hash` is an Argon2id hash in PHC string form. The application role may write it
-- and the lockout counters but never returns it in an answer (it is selected only to verify
-- against, inside a transaction that locks the row).

ALTER TABLE identities ADD COLUMN email text
    CHECK (email IS NULL OR (char_length(email) BETWEEN 3 AND 254 AND email = lower(email)));

CREATE UNIQUE INDEX identities_email_once ON identities (email) WHERE email IS NOT NULL;

ALTER TABLE identities ADD COLUMN password_hash text
    CHECK (password_hash IS NULL OR char_length(password_hash) <= 255);

COMMENT ON COLUMN identities.password_hash IS
'Argon2id hash in PHC string form; selected only to verify against, never returned.';

ALTER TABLE identities ADD COLUMN failed_sign_ins smallint NOT NULL DEFAULT 0
    CHECK (failed_sign_ins >= 0);

ALTER TABLE identities ADD COLUMN locked_until timestamptz;

GRANT UPDATE (email, password_hash, failed_sign_ins, locked_until) ON identities TO colophon_api;
