-- Deleting a project removes its audience and its asset versions too.
--
-- colophon_delete_project (0048) lists a project's tables by hand, and the tables added since —
-- asset_versions (0051), site_roles, site_users, site_sessions (0052), audience_magic_links
-- (0053) and audience_origins (0055) — were never added. Each holds a NO ACTION key to its
-- project or to a parent the function deletes, so deleting any project that had a site user, a
-- magic link, a listed origin or a versioned asset failed on that key: the owner could not
-- delete it, and its visitors' emails and password hashes stayed with it. The function now
-- deletes them, children first, and a catalogue test keeps the list complete.
--
-- The function belongs to colophon_deleter, and only an owner may replace a function — a
-- migrator that isn't a superuser is not one — so it is dropped as its owner and created again
-- exactly as 0048 created it, with the same grants.

SET LOCAL ROLE colophon_deleter;
DROP FUNCTION colophon_delete_project(uuid);
RESET ROLE;

CREATE FUNCTION colophon_delete_project(project uuid)
    RETURNS TABLE (outcome text)
    LANGUAGE plpgsql
    VOLATILE
    SECURITY DEFINER
    SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    actor uuid := nullif(current_setting('app.identity_id', true), '')::uuid;
    slug text;
    title text;
BEGIN
    IF actor IS NULL
       OR project IS DISTINCT FROM nullif(current_setting('app.project_id', true), '')::uuid THEN
        RAISE EXCEPTION 'colophon_delete_project runs only inside its own project''s context'
            USING ERRCODE = 'insufficient_privilege';
    END IF;
    SELECT p.slug, p.name INTO slug, title
    FROM public.projects p WHERE p.id = colophon_delete_project.project;
    IF NOT FOUND THEN
        RETURN QUERY SELECT 'not_found'::text;
        RETURN;
    END IF;
    -- The record first, then the truth of it: the ledger row survives the project; the audit
    -- trail does not, by choice — a deleted project's history is the ledger's, off-host.
    INSERT INTO public.deletion_ledger (kind, project_id, project_slug, environment_id, detail)
    VALUES ('project', colophon_delete_project.project, slug, NULL,
            jsonb_build_object('slug', slug, 'name', title));
    PERFORM set_config('app.deleting_project', colophon_delete_project.project::text, true);
    -- Children before parents, as the foreign keys require. A table a later migration adds in a
    -- project's scope must be added here too: crates/colophon-db/tests/ledger.rs reads the
    -- catalogue and fails on any table with a project_id this neither deletes nor cascades to.
    DELETE FROM public.observations o WHERE o.project_id = colophon_delete_project.project;
    DELETE FROM public.budget_reservations b WHERE b.project_id = colophon_delete_project.project;
    DELETE FROM public.jobs j WHERE j.project_id = colophon_delete_project.project;
    DELETE FROM public.asset_references a WHERE a.project_id = colophon_delete_project.project;
    DELETE FROM public.asset_jobs a WHERE a.project_id = colophon_delete_project.project;
    DELETE FROM public.tombstones t WHERE t.project_id = colophon_delete_project.project;
    DELETE FROM public.publications p WHERE p.project_id = colophon_delete_project.project;
    DELETE FROM public.revision_references r WHERE r.project_id = colophon_delete_project.project;
    DELETE FROM public.unique_claims u WHERE u.project_id = colophon_delete_project.project;
    DELETE FROM public.comments c WHERE c.project_id = colophon_delete_project.project;
    DELETE FROM public.tasks t WHERE t.project_id = colophon_delete_project.project;
    DELETE FROM public.migration_items m WHERE m.project_id = colophon_delete_project.project;
    DELETE FROM public.bulk_items b WHERE b.project_id = colophon_delete_project.project;
    DELETE FROM public.bulk_jobs b WHERE b.project_id = colophon_delete_project.project;
    DELETE FROM public.schedules s WHERE s.project_id = colophon_delete_project.project;
    DELETE FROM public.releases r WHERE r.project_id = colophon_delete_project.project;
    DELETE FROM public.reviews r WHERE r.project_id = colophon_delete_project.project;
    DELETE FROM public.changeset_items c WHERE c.project_id = colophon_delete_project.project;
    DELETE FROM public.changesets c WHERE c.project_id = colophon_delete_project.project;
    DELETE FROM public.review_stages r WHERE r.project_id = colophon_delete_project.project;
    DELETE FROM public.destinations d WHERE d.project_id = colophon_delete_project.project;
    DELETE FROM public.asset_versions a WHERE a.project_id = colophon_delete_project.project;
    DELETE FROM public.assets a WHERE a.project_id = colophon_delete_project.project;
    DELETE FROM public.revisions r WHERE r.project_id = colophon_delete_project.project;
    DELETE FROM public.entries e WHERE e.project_id = colophon_delete_project.project;
    DELETE FROM public.migration_plans m WHERE m.project_id = colophon_delete_project.project;
    DELETE FROM public.content_type_versions c WHERE c.project_id = colophon_delete_project.project;
    DELETE FROM public.collections c WHERE c.project_id = colophon_delete_project.project;
    DELETE FROM public.component_versions c WHERE c.project_id = colophon_delete_project.project;
    DELETE FROM public.components c WHERE c.project_id = colophon_delete_project.project;
    DELETE FROM public.idempotency_records i WHERE i.project_id = colophon_delete_project.project;
    DELETE FROM public.environments e WHERE e.project_id = colophon_delete_project.project;
    DELETE FROM public.api_token_limits a WHERE a.project_id = colophon_delete_project.project;
    DELETE FROM public.api_token_permissions a WHERE a.project_id = colophon_delete_project.project;
    DELETE FROM public.api_tokens a WHERE a.project_id = colophon_delete_project.project;
    DELETE FROM public.audit_events a WHERE a.project_id = colophon_delete_project.project;
    DELETE FROM public.memberships m WHERE m.project_id = colophon_delete_project.project;
    DELETE FROM public.invitations i WHERE i.project_id = colophon_delete_project.project;
    DELETE FROM public.roles r WHERE r.project_id = colophon_delete_project.project;
    DELETE FROM public.site_sessions s WHERE s.project_id = colophon_delete_project.project;
    DELETE FROM public.site_users s WHERE s.project_id = colophon_delete_project.project;
    DELETE FROM public.site_roles s WHERE s.project_id = colophon_delete_project.project;
    DELETE FROM public.audience_magic_links l WHERE l.project_id = colophon_delete_project.project;
    DELETE FROM public.audience_origins o WHERE o.project_id = colophon_delete_project.project;
    DELETE FROM public.projects p WHERE p.id = colophon_delete_project.project;
    -- `app.deleting_project` deliberately stays set: the deferred last-owner trigger reads it
    -- at commit. The setting is transaction-local, so it cannot leak to another transaction,
    -- and it names this project only — every guard keeps its force for everything else.
    RETURN QUERY SELECT 'deleted'::text;
END
$$;

ALTER FUNCTION colophon_delete_project(uuid) OWNER TO colophon_deleter;
REVOKE ALL ON FUNCTION colophon_delete_project(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION colophon_delete_project(uuid) TO colophon_api;

-- colophon_api can't delete asset versions or site users (they are kept by their own paths);
-- the deleter can, for this function only — nothing logs in as it.
GRANT DELETE ON asset_versions, site_sessions, site_users, site_roles, audience_magic_links,
    audience_origins
    TO colophon_deleter;
