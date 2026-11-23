-- D-233 (the independent review's S2-2, 2026-11-21): the media signing key gets an id and can be rotated.
--
-- 0082 made one key per installation, read once per server and never written again. A key that leaked (from a
-- backup, or a staging copy restored from production, which kept production's key) signed any path in any project
-- for as long as the installation lived. Now each key has an id that signed addresses carry (`?kid=`); at most
-- two keys live, the current one (the highest id), which signs, and the one before it, which still verifies the
-- links signed in its last minutes. `colophon admin media-key rotate` makes a new one (and, with
-- `--retire-previous`, drops every other), and every restore's epoch stage does the same, so a restored copy never
-- shares a key with the installation it came from.
--
-- 0082's row keeps its id, 1, so the links signed before this migration fail only because their message format
-- changed (v1 to v2), not because the key did.

ALTER TABLE media_signing_keys DROP CONSTRAINT media_signing_keys_id_check;
ALTER TABLE media_signing_keys ADD CONSTRAINT media_signing_keys_id_check CHECK (id > 0);

-- Every rotation, installation-wide (no project's audit log applies): which key it made, when, the database login
-- that ran it, whether it retired the key before, and why (`admin`, `restore`). Never the key. Append-only.
CREATE TABLE media_key_rotations (
    kid smallint PRIMARY KEY,
    at timestamptz NOT NULL DEFAULT now(),
    by_role text NOT NULL DEFAULT current_user,
    retired_previous boolean NOT NULL,
    reason text NOT NULL CHECK (reason IN ('admin', 'restore'))
);
CREATE TRIGGER media_key_rotations_append_only BEFORE UPDATE OR DELETE ON media_key_rotations
    FOR EACH ROW EXECUTE FUNCTION colophon_audit_append_only();
CREATE TRIGGER media_key_rotations_no_truncate BEFORE TRUNCATE ON media_key_rotations
    FOR EACH STATEMENT EXECUTE FUNCTION colophon_audit_append_only();

-- A new current key, retiring every key but the one it replaces (or, with p_retire_previous, every other), and its
-- line in media_key_rotations: the one write path, run by the migrator's login (the admin command, the restore
-- stage). 32 bytes from two version-4 UUIDs' random bits (244 of them), as 0082 made the first.
CREATE FUNCTION colophon_rotate_media_key(p_retire_previous boolean, p_reason text)
RETURNS smallint
LANGUAGE plpgsql SECURITY INVOKER SET search_path = public, pg_temp AS $$
DECLARE
  made smallint;
BEGIN
  -- Two rotations at once take turns (SHARE ROW EXCLUSIVE conflicts with itself): the second waits, then sees the
  -- first's key as the one it replaces, so neither inserts the same id nor deletes the other's key.
  LOCK TABLE media_signing_keys IN SHARE ROW EXCLUSIVE MODE;
  INSERT INTO media_signing_keys (id, key)
  SELECT coalesce(max(id), 0) + 1,
         decode(replace(gen_random_uuid()::text || gen_random_uuid()::text, '-', ''), 'hex')
    FROM media_signing_keys
  RETURNING id INTO made;
  DELETE FROM media_signing_keys
   WHERE id < CASE WHEN p_retire_previous THEN made ELSE made - 1 END;
  INSERT INTO media_key_rotations (kid, retired_previous, reason) VALUES (made, p_retire_previous, p_reason);
  RETURN made;
END $$;

REVOKE ALL ON FUNCTION colophon_rotate_media_key(boolean, text) FROM PUBLIC;
