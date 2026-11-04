// Query keys and readers shared by the shell and the pages, so one fetch serves both and one
// invalidation refreshes both (D-030).

import {
	getProject,
	listChangesets,
	listComments,
	listComponents,
	listContentTypes,
	listDestinations,
	listMembers,
	listMigrations,
	listReleases,
	listRoles,
	listTasks,
	me,
	searchEntries
} from './api.ts';

export const projectQuery = (project: string) => ({ queryKey: ['project', project], queryFn: () => getProject(project) });
export const typesQuery = (project: string) => ({ queryKey: ['types', project], queryFn: () => listContentTypes(project) });
export const meQuery = () => ({ queryKey: ['me'], queryFn: me });
export const changesetsQuery = (project: string) => ({ queryKey: ['changesets', project], queryFn: () => listChangesets(project) });
export const releasesQuery = (project: string) => ({ queryKey: ['releases', project], queryFn: () => listReleases(project) });
export const rolesQuery = (project: string) => ({ queryKey: ['roles', project], queryFn: () => listRoles(project) });
export const componentsQuery = (project: string) => ({ queryKey: ['components', project], queryFn: () => listComponents(project) });
export const destinationsQuery = (project: string) => ({ queryKey: ['destinations', project], queryFn: () => listDestinations(project) });
export const migrationsQuery = (project: string) => ({ queryKey: ['migrations', project], queryFn: () => listMigrations(project) });
/** One collection's newest entries, or those whose title matches `q`. */
export const entriesQuery = (project: string, collection: string, q = '') => ({
	queryKey: ['entries', project, collection, q.trim()],
	queryFn: () => searchEntries(project, collection, q)
});

/** The project's members, for assignee pickers and naming people (E1); the same key the Members
 *  page uses, so one fetch serves both and its invalidation refreshes both. */
export const membersQuery = (project: string) => ({ queryKey: ['members', project], queryFn: () => listMembers(project) });
/** An entry's comments (E1): `state` narrows to the open or the resolved ones. */
export const commentsQuery = (project: string, collection: string, entry: string, state?: 'open' | 'resolved') => ({
	queryKey: ['comments', project, collection, entry, state ?? ''],
	queryFn: () => listComments(project, collection, entry, state === undefined ? {} : { state })
});
/** The environment's tasks (E1), filtered the way the API filters them. */
export const tasksQuery = (
	project: string,
	filter: { entry?: string; collection?: string; assignee?: string; status?: 'open' | 'done' | 'cancelled' } = {}
) => ({ queryKey: ['tasks', project, filter], queryFn: () => listTasks(project, filter) });

/** "12" or, when there are more than a page, "100+". */
export const count = (page: { data: unknown[]; next_cursor: string | null }) =>
	page.next_cursor === null ? String(page.data.length) : `${page.data.length}+`;

/** "3 minutes ago", "yesterday", "12 Oct". */
export function ago(at: string, now = Date.now()): string {
	const seconds = Math.round((now - new Date(at).getTime()) / 1000);
	const words = new Intl.RelativeTimeFormat(undefined, { numeric: 'auto' });
	if (seconds < 45) return 'just now';
	if (seconds < 3600) return words.format(-Math.round(seconds / 60), 'minute');
	if (seconds < 86_400) return words.format(-Math.round(seconds / 3600), 'hour');
	if (seconds < 7 * 86_400) return words.format(-Math.round(seconds / 86_400), 'day');
	return new Date(at).toLocaleDateString(undefined, { day: 'numeric', month: 'short' });
}
