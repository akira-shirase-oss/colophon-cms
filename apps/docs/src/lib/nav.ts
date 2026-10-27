// The site map: every guide page, in reading order, with its capability label. The left
// navigation, the previous/next links, each page's title and label, and the link checker all read
// this one list. The design documents are listed from the repository's own files instead
// (lib/server/design-docs.ts), so adding a document there adds it here.

export const REPOSITORY = 'https://github.com/akira-shirase-oss/colophon-cms';

/**
 * What a page describes, honestly (module 16, DS1):
 * - `implemented`: built, with tests, in the code this site is built from;
 * - `building`: a step in progress on its own branch, not in this code yet;
 * - `planned`: designed, not built, with the milestone it belongs to;
 * - `overview`: a page that introduces others, each labelled on its own;
 * - `design`: a design document from `docs/`, which mixes what is built and what is planned.
 */
export type Capability =
	| { kind: 'implemented' }
	| { kind: 'building'; step: string }
	| { kind: 'planned'; milestone: string }
	| { kind: 'overview' }
	| { kind: 'design' };

export interface DocPage {
	/** The page's address, with a trailing slash. */
	href: string;
	title: string;
	/** One sentence for search results, link cards and `<meta name="description">`. */
	description: string;
	capability: Capability;
	/** The file the page is written in, from the repository's root. */
	source: string;
}

export interface Section {
	title: string;
	href: string;
	pages: DocPage[];
}

const implemented: Capability = { kind: 'implemented' };
const overview: Capability = { kind: 'overview' };

function md(href: string, title: string, description: string, capability: Capability): DocPage {
	return { href, title, description, capability, source: `apps/docs/src/routes${href}+page.md` };
}

function svelte(href: string, title: string, description: string, capability: Capability): DocPage {
	return { href, title, description, capability, source: `apps/docs/src/routes${href}+page.svelte` };
}

export const HOME = svelte('/', 'Colophon documentation', 'Guides, concepts and references for Colophon CMS, a Rust and Svelte CMS that can prove what it published.', overview);

export const SECTIONS: Section[] = [
	{
		title: 'Get started',
		href: '/get-started/',
		pages: [
			md('/get-started/', 'What Colophon is', 'A CMS that can prove what it published, built in the open; pre-release, v0.1 is not out.', overview),
			md('/get-started/quickstart/', 'Developer quickstart', 'Run Colophon on your machine: the local stack, the server, the Studio and the CLI.', implemented),
			md('/get-started/status/', 'Status and roadmap', 'What is built, what is next, and how each milestone is proven.', overview)
		]
	},
	{
		title: 'For editors',
		href: '/editors/',
		pages: [
			md('/editors/', 'Editor guide', 'Writing, review and publishing in the Studio, in plain words.', overview),
			md('/editors/joining/', 'Joining a project', 'Signing in, accepting an invitation link, and what each role may do.', implemented),
			md('/editors/writing/', 'Writing and autosave', 'How the Studio saves your work, what its save messages mean, and what happens when two people edit.', implemented),
			md('/editors/versions/', 'Versions and comparing them', 'Every save is a version you can look back at and compare with your draft.', implemented),
			md('/editors/review/', 'Asking for review', 'Gather entries into a publish request, send it for review, and what makes an approval run out.', implemented),
			md('/editors/publishing/', 'Publishing', 'Publishing an approved request, and what happens when someone else published first.', implemented),
			md('/editors/website-status/', 'What the website labels mean', '"Published in CMS", "Website update pending", "Verified at 1 location" and "Needs attention".', implemented),
			md('/editors/unpublishing/', 'Unpublishing', 'Taking an entry off the website, and what can and cannot be taken back.', implemented)
		]
	},
	{
		title: 'Concepts',
		href: '/concepts/',
		pages: [
			md('/concepts/', 'Concepts', 'How Colophon keeps its promises, one idea at a time.', overview),
			md('/concepts/drafts-and-revisions/', 'Drafts and revisions', 'Every save is a new, never-changed revision; the draft is a pointer to the latest.', implemented),
			md('/concepts/saves-and-conflicts/', 'Saves, If-Match and conflicts', 'A save names the revision it builds on, so a stale save is refused, never merged silently.', implemented),
			md('/concepts/content-types/', 'Content types and safe changes', 'Versioned definitions with six field types, changed only in ways that keep stored entries valid.', implemented),
			md('/concepts/roles/', 'Roles and invitations', 'Four roles decided by one policy table, single-use invitation links, and the first administrator.', implemented),
			md('/concepts/changesets-and-reviews/', 'Changesets and hash-bound reviews', 'An approval counts only for the exact revisions it was given on.', implemented),
			md('/concepts/releases-and-generations/', 'Releases and generations', 'A publish is one transaction that takes the next generation; generations only move forward.', implemented),
			md('/concepts/outbox-and-worker/', 'The outbox and the worker', 'External work is committed as jobs with the release, then done by a leased worker with a stable key.', implemented),
			md('/concepts/delivery-cache/', 'The delivery cache', 'Public answers cached under keys that name their generation, which always comes from Postgres.', implemented),
			md('/concepts/unpublish-and-revocation/', 'Unpublish and revocation', 'Tombstones, revocation through historical URLs, and CDN purges that report what they could not prove.', implemented),
			md('/concepts/receipts-and-probes/', 'Delivery receipts and probes', 'Each release gets one label, and "verified" always says at how many locations.', implemented),
			md('/concepts/simulator/', 'The deterministic simulator', 'Thousands of seeded runs with crashes and network faults; a failure is a seed you can replay.', implemented)
		]
	},
	{
		title: 'Reference',
		href: '/reference/',
		pages: [
			md('/reference/', 'Reference', 'The HTTP APIs, the CLI, the rich-text format and webhooks.', overview),
			md('/reference/api/', 'Management API conventions', 'Authentication, response shapes, If-Match and ETags, idempotency keys, pagination and status codes.', implemented),
			md('/reference/api/routes/', 'Management API routes', 'Every route the server answers today, with who may call it.', implemented),
			md('/reference/api/errors/', 'Errors', 'The error envelope, its stable codes and the reasons in details.', implemented),
			md('/reference/delivery-api/', 'Delivery API', 'The public, read-only /content/v1 API: latest and pinned reads, ETags, cache headers and 410.', implemented),
			md('/reference/cli/', 'Command line', 'The colophon command: sign-in, content, the operator commands, JSON output and exit codes.', implemented),
			md('/reference/rich-text/', 'Rich-text format', 'colophon/rich-text v1: the stored JSON of every rich-text field, its limits and error codes.', implemented),
			md('/reference/webhooks/', 'Webhooks and website connections', 'Destinations (webhooks, CDN purges and probes), the release.committed payload and its headers.', implemented)
		]
	},
	{
		title: 'Operations',
		href: '/operations/',
		pages: [
			md('/operations/', 'Operations', 'Running Colophon locally, configuring it, and the evidence behind it.', overview),
			md('/operations/local-stack/', 'The local stack', 'PostgreSQL, Valkey, Garage and Keycloak on 127.0.0.1, and the checks that prove each works.', implemented),
			md('/operations/configuration/', 'Configuration', 'Every COLOPHON_* setting the server reads, with its default.', implemented),
			svelte('/operations/load-testing/', 'Load testing', 'scripts/load.sh, and every result kept in perf/results, with the machine it ran on.', implemented),
			md('/operations/crash-tests/', 'Crash tests and the crash loop', 'Fault points, the real-process crash loop and the simulator sweeps that CI runs.', implemented),
			md('/operations/backup-and-upgrades/', 'Backups, restore and upgrades', 'What the production runbooks will cover, and why none of it exists yet.', { kind: 'planned', milestone: 'P6' })
		]
	}
];

/** The design documents' section; its pages come from the repository (see the root layout). */
export const DESIGN = { title: 'Design documents', href: '/design/' } as const;

export const PAGES: DocPage[] = SECTIONS.flatMap((section) => section.pages);

/** The page at an address, if the site map has it. */
export function pageAt(pathname: string): DocPage | undefined {
	const path = pathname.endsWith('/') ? pathname : `${pathname}/`;
	return path === '/' ? HOME : PAGES.find((page) => page.href === path);
}

/** The pages before and after one, in reading order. */
export function neighbours(order: { href: string; title: string }[], href: string) {
	const index = order.findIndex((page) => page.href === href);
	return {
		previous: index > 0 ? order[index - 1] : undefined,
		next: index >= 0 && index < order.length - 1 ? order[index + 1] : undefined
	};
}

export function editUrl(source: string): string {
	return `${REPOSITORY}/edit/main/${source}`;
}

export function blobUrl(path: string): string {
	return `${REPOSITORY}/blob/main/${path}`;
}
