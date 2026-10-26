-- Milestone P1, step 1.6: signing in from a terminal. docs/modules/03-auth.md ("Machine
-- credentials", the CLI part), D-041.
--
-- A command-line client asks for a device login, a person approves it in a browser where they
-- are already signed in, and the client exchanges it once for a personal access token. Like
-- session tokens, the device secret and the access token are stored only as SHA-256 hashes.

CREATE TABLE device_logins (
    device_hash bytea PRIMARY KEY CHECK (length(device_hash) = 32),
    user_code text NOT NULL UNIQUE CHECK (user_code ~ '^[BCDFGHJKLMNPQRSTVWXZ2-9]{4}-[BCDFGHJKLMNPQRSTVWXZ2-9]{4}$'),
    client_name text NOT NULL CHECK (char_length(client_name) BETWEEN 1 AND 100),
    approved_by uuid REFERENCES identities (id),
    approved_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL CHECK (expires_at > created_at),
    CHECK ((approved_by IS NULL) = (approved_at IS NULL))
);
CREATE INDEX device_logins_by_expiry ON device_logins (expires_at);

CREATE TABLE access_tokens (
    id uuid PRIMARY KEY,
    token_hash bytea NOT NULL UNIQUE CHECK (length(token_hash) = 32),
    identity_id uuid NOT NULL REFERENCES identities (id),
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 100),
    created_at timestamptz NOT NULL DEFAULT now(),
    last_used_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL CHECK (expires_at > created_at),
    revoked_at timestamptz
);
CREATE INDEX access_tokens_by_identity ON access_tokens (identity_id);

-- Device logins are consumed by deleting them; tokens are revoked, never deleted.
GRANT SELECT, INSERT, DELETE, UPDATE (approved_by, approved_at) ON device_logins TO colophon_api;
GRANT SELECT, INSERT, UPDATE (last_used_at, revoked_at) ON access_tokens TO colophon_api;
