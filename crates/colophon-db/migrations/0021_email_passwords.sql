-- Email-and-password sign-in (the invitation flow): an identity created with a password is
-- `issuer = 'password'`, `subject = email`, so the existing (issuer, subject) uniqueness
-- already gives one account per email. `email` is kept normalized lowercase by the API; the
-- unique index makes that rule physical. `password_hash` is an Argon2id PHC string — it is
-- on the same row the app reads, and only ever compared, never returned by any endpoint.
-- Sign-in is bounded: `failed_sign_ins` counts consecutive misses, `locked_until` pauses
-- the account for a while once it passes the threshold.
ALTER TABLE identities ADD COLUMN email text
    CHECK (email IS NULL OR (char_length(email) BETWEEN 3 AND 254 AND email = lower(email)));
CREATE UNIQUE INDEX identities_email_once ON identities (email) WHERE email IS NOT NULL;
ALTER TABLE identities ADD COLUMN password_hash text CHECK (char_length(password_hash) <= 255);
ALTER TABLE identities ADD COLUMN failed_sign_ins smallint NOT NULL DEFAULT 0;
ALTER TABLE identities ADD COLUMN locked_until timestamptz;
COMMENT ON COLUMN identities.password_hash IS 'An Argon2id PHC string; compared on sign-in, never returned.';

-- Sign-in and sign-up reach these columns only through the identity functions; the hash is
-- writable for the password-set and the counters for the lockout.
GRANT SELECT, INSERT, UPDATE (email, password_hash, failed_sign_ins, locked_until)
    ON identities TO colophon_api;
