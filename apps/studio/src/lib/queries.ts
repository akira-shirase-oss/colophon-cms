// Query keys and readers shared by the shell and the pages, so one fetch serves both and one
// invalidation refreshes both (D-030).

import {
	getProject,
	listChangesets,
	listComponents,
	listContentTypes,
	listDestinations,
	listReleases,
	listRoles,
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
/** One collection's newest entries, or those whose title matches `q`. */
export const entriesQuery = (project: string, collection: string, q = '') => ({
	queryKey: ['entries', project, collection, q.trim()],
	queryFn: () => searchEntries(project, collection, q)
});

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
