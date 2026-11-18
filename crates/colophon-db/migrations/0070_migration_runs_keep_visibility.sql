-- D-183: a content-migration run keeps a gated entry gated.
--
-- 0041's `colophon_migration_apply_item` writes the migrated revision in SQL and names no
-- `visibility` — it predates the column (0052) — so the revision took the column's default,
-- `public`, while the entry's `draft_visibility` stayed gated. The next publish copied the
-- revision's `public` into the projection, and a member-only entry answered anonymous readers.
--
-- The run's revision replaces the draft it was planned on (`base_revision_id`, the `expected`
-- the function checked under the entry's lock), so it takes that revision's gate. A trigger
-- rather than a redefinition of the function: it is SECURITY DEFINER and owned by
-- `colophon_migrator`, so `current_user` is that role inside it, and the application's own
-- writes — which always name their gate — are untouched.
--
-- Revisions a run already wrote, and the publications made from them, take their base's gate
-- here too.

CREATE FUNCTION colophon_migration_revision_visibility() RETURNS trigger
    LANGUAGE plpgsql
    SET search_path = pg_catalog, public
AS $$
BEGIN
    IF current_user = 'colophon_migrator' AND NEW.base_revision_id IS NOT NULL THEN
        NEW.visibility := coalesce(
            (SELECT b.visibility FROM public.revisions b
             WHERE b.project_id = NEW.project_id AND b.environment_id = NEW.environment_id
               AND b.id = NEW.base_revision_id),
            NEW.visibility);
    END IF;
    RETURN NEW;
END
$$;
CREATE TRIGGER revisions_migration_keeps_visibility BEFORE INSERT ON revisions
    FOR EACH ROW EXECUTE FUNCTION colophon_migration_revision_visibility();

ALTER TABLE revisions NO FORCE ROW LEVEL SECURITY;
ALTER TABLE publications NO FORCE ROW LEVEL SECURITY;
UPDATE revisions r SET visibility = b.visibility
FROM migration_items i
JOIN revisions b ON b.project_id = i.project_id AND b.environment_id = i.environment_id
                AND b.entry_id = i.entry_id
WHERE r.project_id = i.project_id AND r.environment_id = i.environment_id
  AND r.id = i.new_revision_id AND b.id = r.base_revision_id
  AND r.visibility <> b.visibility;
UPDATE publications p SET visibility = r.visibility
FROM migration_items i
JOIN revisions r ON r.project_id = i.project_id AND r.environment_id = i.environment_id
                AND r.id = i.new_revision_id
WHERE p.project_id = i.project_id AND p.environment_id = i.environment_id
  AND p.revision_id = r.id AND p.visibility <> r.visibility;
ALTER TABLE revisions FORCE ROW LEVEL SECURITY;
ALTER TABLE publications FORCE ROW LEVEL SECURITY;
