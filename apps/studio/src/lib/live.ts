// Live lists (D-108, step E3): the Studio holds one `GET {P}/events` stream open per project
// and turns each `change` hint into the invalidations a fresh visit would run, so a list moves
// as PostgreSQL commits — a hint names what changed and never carries content, so what follows
// is a refetch through the API the role was already authorized for.
//
// - `resync` (the receiver lagged, or the server's listener reconnected) and any reconnect
//   invalidate every project-scoped key: the stream is a disposable accelerator and Postgres
//   stays the authority, so a missed hint costs one refetch, never a wrong row.
// - The stream ends cleanly at its server-side lifetime; EventSource re-opens it on its own,
//   and an open after a gap may have missed hints, so it resyncs rather than trust the state.
// - A backgrounded tab can lose its connection quietly: becoming visible re-opens a dead
//   stream and resyncs either way.

import type { QueryClient } from '@tanstack/svelte-query';

/** A `change` event's data: `{"k":"<kind>","c":"<collection>"?,"e":"<environment>"?}`. */
interface Hint {
	k?: string;
	c?: string;
	e?: string;
}

/** The query keys one kind's change makes stale. Keys are the ones `queries.ts` and the pages
 *  use; a two-part key matches every deeper query on the same prefix (`['entries', p, c]`
 *  covers each search and state read of the collection). Single-read keys (`['draft', …]`,
 *  `['changeset', …]`, `['migration', …]`, `['receipt', …]`) are included so the page open on
 *  that one thing moves too — except `['type', …]` and `['component', …]`, which the builders
 *  key on version: refetching them mid-edit would remount the builder and drop its work. */
const KIND_KEYS: Record<string, (project: string, collection?: string) => string[][]> = {
	entries: (project, collection) => [
		collection === undefined ? ['entries', project] : ['entries', project, collection],
		['draft', project],
		['history', project],
		['linked', project]
	],
	types: (project) => [['types', project]],
	components: (project) => [
		['types', project],
		['components', project]
	],
	changesets: (project) => [
		['changesets', project],
		['changeset', project]
	],
	stages: (project) => [
		['stages', project],
		['changesets', project],
		['changeset', project]
	],
	// Jobs and probes report under `releases`: the release list, its deliveries and an open
	// receipt all settle live.
	releases: (project) => [
		['releases', project],
		['jobs', project],
		['receipt', project]
	],
	schedules: (project) => [['schedules', project]],
	tasks: (project) => [['tasks', project]],
	comments: (project) => [['comments', project]],
	migrations: (project) => [
		['migrations', project],
		['migration', project],
		['migration-items', project],
		['types', project]
	],
	assets: (project) => [
		['assets', project],
		['asset', project]
	],
	members: (project) => [
		['members', project],
		['invitations', project]
	],
	roles: (project) => [
		['roles', project],
		['project', project]
	],
	tokens: (project) => [['tokens', project]],
	destinations: (project) => [['destinations', project]],
	audit: (project) => [['audit', project]]
};

/**
 * Holds a `{P}/events` stream for `project` in `env`, invalidating the project's queries as
 * hints land. Returns the teardown: closes the stream and the visibility listener.
 */
export function watchProject(project: string, env: string, queryClient: QueryClient): () => void {
	const url = `/api/v1/projects/${project}/environments/${env}/events`;

	/** Every key scoped to this project: `['*', project, …]` — the resync answer. */
	const resync = () =>
		void queryClient.invalidateQueries({
			predicate: (query) => Array.isArray(query.queryKey) && query.queryKey[1] === project
		});

	let source: EventSource | null = null;
	let opened = false;

	function onChange(event: MessageEvent<string>) {
		let hint: Hint;
		try {
			hint = JSON.parse(event.data) as Hint;
		} catch {
			return; // A frame that doesn't read is not a reason to distrust the state.
		}
		for (const key of KIND_KEYS[hint.k ?? '']?.(project, hint.c) ?? []) {
			void queryClient.invalidateQueries({ queryKey: key });
		}
	}

	function connect() {
		const stream = new EventSource(url);
		source = stream;
		stream.addEventListener('change', onChange);
		stream.addEventListener('resync', resync);
		stream.addEventListener('open', () => {
			// The first open trusts the page's own loads; a later one may have missed hints.
			if (opened) resync();
			opened = true;
		});
		// `error` needs no handler: EventSource retries on its own, and the next `open` resyncs.
	}

	function onVisibility() {
		if (document.visibilityState !== 'visible') return;
		if (source !== null && source.readyState !== EventSource.OPEN) {
			source.close();
			connect();
		}
		resync();
	}

	connect();
	document.addEventListener('visibilitychange', onVisibility);

	return () => {
		document.removeEventListener('visibilitychange', onVisibility);
		source?.close();
		source = null;
	};
}
