-- Milestone E, step E3: committed changes raise a hint (D-108, docs/modules/02-db.md "Change
-- notifications", 04-api.md `{P}/events`).
--
-- A server that serves `GET {P}/events` holds one LISTEN connection on `colophon_events` and
-- fans the payloads out to the streams of that project. The hint names what changed and nothing
-- more — project, environment (or `--` for the project's own rows), kind, and the collection for
-- the kinds that have one — so a client refetches through the API it was already authorized for,
-- and PostgreSQL stays the only authority. Because `pg_notify` fires inside the trigger's
-- transaction, a hint exists only for a commit: rolled-back work notifies nothing, and
-- PostgreSQL folds identical payloads of one transaction into one notification, so saving ten
-- entries of one collection lands as one `entries|articles` hint.
--
-- Payload: `p=<project uuid>|e=<environment uuid|-->|k=<kind>[|c=<collection>]`. The trigger's
-- arguments are the kind and the collection's column name (`''` when the kind has none); every
-- watched table has `project_id`, and the environment reads `environment_id`, which project-level
-- tables lack — for them `e` is `--`. Kinds: entries, types, components, changesets, stages,
-- releases, schedules, tasks, comments, migrations, assets, members, roles, tokens,
-- destinations, audit. Table-to-kind: a review or a changeset item is a `changesets` change; a
-- delivery job, an unpublish tombstone or a probe observation is `releases`; a publication is an
-- `entries` change for the entry's collection (it changes what readers get); an invitation is
-- `members`; a permission row is `roles`/`tokens`. Not watched: identity and sign-in tables (no
-- live view), the outbox's own bookkeeping, `environments` and `projects` (a member learns them
-- on the next read — they don't change under a live list), and `revisions`, whose every insert
-- already moves `entries` in the same transaction.

CREATE FUNCTION colophon_notify() RETURNS trigger
    LANGUAGE plpgsql AS $$
DECLARE
    changed jsonb;
BEGIN
    IF TG_OP = 'DELETE' THEN
        changed := to_jsonb(OLD);
    ELSE
        changed := to_jsonb(NEW);
    END IF;
    PERFORM pg_notify(
        'colophon_events',
        'p=' || (changed ->> 'project_id')
            || '|e=' || coalesce(changed ->> 'environment_id', '--')
            || '|k=' || TG_ARGV[0]
            -- `|c=<collection>` only for kinds that carry one (entries, comments, tasks).
            || coalesce('|c=' || nullif(changed ->> TG_ARGV[1], ''), '')
    );
    RETURN NULL;
END;
$$;

COMMENT ON FUNCTION colophon_notify() IS
    'E3 (D-108): pg_notify a compact change hint on colophon_events for the live-list bus.';

-- Environment-scoped tables. Entries' hint follows publications and unpublish tombstones too:
-- both change what a list shows as live.
CREATE TRIGGER entries_notify AFTER INSERT OR UPDATE OR DELETE ON entries
    FOR EACH ROW EXECUTE FUNCTION colophon_notify('entries', 'collection');
CREATE TRIGGER publications_notify AFTER INSERT OR UPDATE OR DELETE ON publications
    FOR EACH ROW EXECUTE FUNCTION colophon_notify('entries', 'collection');
CREATE TRIGGER tombstones_notify AFTER INSERT ON tombstones
    FOR EACH ROW EXECUTE FUNCTION colophon_notify('entries', 'collection');
CREATE TRIGGER collections_notify AFTER INSERT OR UPDATE OR DELETE ON collections
    FOR EACH ROW EXECUTE FUNCTION colophon_notify('types', '');
CREATE TRIGGER content_type_versions_notify AFTER INSERT OR UPDATE OR DELETE ON content_type_versions
    FOR EACH ROW EXECUTE FUNCTION colophon_notify('types', '');
CREATE TRIGGER components_notify AFTER INSERT OR UPDATE OR DELETE ON components
    FOR EACH ROW EXECUTE FUNCTION colophon_notify('components', '');
CREATE TRIGGER component_versions_notify AFTER INSERT OR UPDATE OR DELETE ON component_versions
    FOR EACH ROW EXECUTE FUNCTION colophon_notify('components', '');
CREATE TRIGGER changesets_notify AFTER INSERT OR UPDATE OR DELETE ON changesets
    FOR EACH ROW EXECUTE FUNCTION colophon_notify('changesets', '');
CREATE TRIGGER changeset_items_notify AFTER INSERT OR UPDATE OR DELETE ON changeset_items
    FOR EACH ROW EXECUTE FUNCTION colophon_notify('changesets', '');
CREATE TRIGGER reviews_notify AFTER INSERT OR UPDATE OR DELETE ON reviews
    FOR EACH ROW EXECUTE FUNCTION colophon_notify('changesets', '');
CREATE TRIGGER review_stages_notify AFTER INSERT OR UPDATE OR DELETE ON review_stages
    FOR EACH ROW EXECUTE FUNCTION colophon_notify('stages', '');
CREATE TRIGGER releases_notify AFTER INSERT OR UPDATE OR DELETE ON releases
    FOR EACH ROW EXECUTE FUNCTION colophon_notify('releases', '');
CREATE TRIGGER jobs_notify AFTER INSERT OR UPDATE OR DELETE ON jobs
    FOR EACH ROW EXECUTE FUNCTION colophon_notify('releases', '');
CREATE TRIGGER observations_notify AFTER INSERT ON observations
    FOR EACH ROW EXECUTE FUNCTION colophon_notify('releases', '');
CREATE TRIGGER schedules_notify AFTER INSERT OR UPDATE OR DELETE ON schedules
    FOR EACH ROW EXECUTE FUNCTION colophon_notify('schedules', '');
CREATE TRIGGER tasks_notify AFTER INSERT OR UPDATE OR DELETE ON tasks
    FOR EACH ROW EXECUTE FUNCTION colophon_notify('tasks', 'collection');
CREATE TRIGGER comments_notify AFTER INSERT OR UPDATE OR DELETE ON comments
    FOR EACH ROW EXECUTE FUNCTION colophon_notify('comments', 'collection');
CREATE TRIGGER migration_plans_notify AFTER INSERT OR UPDATE OR DELETE ON migration_plans
    FOR EACH ROW EXECUTE FUNCTION colophon_notify('migrations', '');
CREATE TRIGGER migration_items_notify AFTER INSERT OR UPDATE OR DELETE ON migration_items
    FOR EACH ROW EXECUTE FUNCTION colophon_notify('migrations', '');
CREATE TRIGGER assets_notify AFTER INSERT OR UPDATE OR DELETE ON assets
    FOR EACH ROW EXECUTE FUNCTION colophon_notify('assets', '');
CREATE TRIGGER asset_jobs_notify AFTER INSERT OR UPDATE OR DELETE ON asset_jobs
    FOR EACH ROW EXECUTE FUNCTION colophon_notify('assets', '');
CREATE TRIGGER asset_references_notify AFTER INSERT OR UPDATE OR DELETE ON asset_references
    FOR EACH ROW EXECUTE FUNCTION colophon_notify('assets', '');
CREATE TRIGGER destinations_notify AFTER INSERT OR UPDATE OR DELETE ON destinations
    FOR EACH ROW EXECUTE FUNCTION colophon_notify('destinations', '');

-- The project's own rows have no environment_id: their hints carry `e=--` and reach every
-- environment's stream of the project.
CREATE TRIGGER memberships_notify AFTER INSERT OR UPDATE OR DELETE ON memberships
    FOR EACH ROW EXECUTE FUNCTION colophon_notify('members', '');
CREATE TRIGGER invitations_notify AFTER INSERT OR UPDATE OR DELETE ON invitations
    FOR EACH ROW EXECUTE FUNCTION colophon_notify('members', '');
CREATE TRIGGER roles_notify AFTER INSERT OR UPDATE OR DELETE ON roles
    FOR EACH ROW EXECUTE FUNCTION colophon_notify('roles', '');
CREATE TRIGGER role_permissions_notify AFTER INSERT OR UPDATE OR DELETE ON role_permissions
    FOR EACH ROW EXECUTE FUNCTION colophon_notify('roles', '');
CREATE TRIGGER api_tokens_notify AFTER INSERT OR UPDATE OR DELETE ON api_tokens
    FOR EACH ROW EXECUTE FUNCTION colophon_notify('tokens', '');
CREATE TRIGGER api_token_permissions_notify AFTER INSERT OR UPDATE OR DELETE ON api_token_permissions
    FOR EACH ROW EXECUTE FUNCTION colophon_notify('tokens', '');
-- audit_events is append-only by its own triggers, so only INSERT can ever fire.
CREATE TRIGGER audit_events_notify AFTER INSERT ON audit_events
    FOR EACH ROW EXECUTE FUNCTION colophon_notify('audit', '');
