-- D-225: a change to someone's membership waits for their reads, as it waits for their writes.
--
-- A member's write request share-locks their membership row (members::current_policy), so a role
-- change or a removal waits for it, and no request acts under the old role once the change has
-- committed. A read now runs read-only on the read pool, where a row lock isn't allowed, so it
-- holds an advisory lock instead, shared, keyed by the project and the identity (class 0x4D42,
-- "MB", scope::MEMBER_LOCK_CLASS), from its transaction's first statement. Every update or delete
-- of a membership takes that lock exclusive first, by this trigger, so it waits for those reads,
-- and the reads after it see the change. Only read-only transactions take the shared lock: they
-- take no row locks, so they can't be part of a lock cycle; a writer keeps its row lock.

CREATE FUNCTION colophon_membership_lock() RETURNS trigger
    LANGUAGE plpgsql
    SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    PERFORM pg_advisory_xact_lock(19778, hashtext(OLD.project_id::text || '/' || OLD.identity_id::text));
    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER memberships_lock BEFORE UPDATE OR DELETE ON memberships
    FOR EACH ROW EXECUTE FUNCTION colophon_membership_lock();
