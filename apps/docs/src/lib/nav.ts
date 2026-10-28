// The site map: every guide page, in reading order, with its status. The left navigation, the
// previous/next links, each page's title and label, the link checker and the search index all read
// this one list. The design documents, release notes and upgrade guides are listed from the
// repository's own files instead (lib/server/repository-docs.ts), so adding a file there adds it here.

export const REPOSITORY = 'https://github.com/akira-shirase-oss/colophon-cms';

/**
 * What a page describes, in the website's three words (module 16, DS1; apps/site/src/lib/status.ts):
 * - `done`: merged into main, with its tests, testbed page and load scenario (`at`: the step);
 * - `building`: a step in progress on its own branch, not merged yet (`at`: the step or branch);
 * - `planned`: in the roadmap, nothing built yet (`at`: the milestone);
 * - `overview`: a page that introduces others, each labelled on its own;
 * - `design`: a design document from `docs/`, which mixes what is built and what is planned.
 */
export type Capability =
	| { kind: 'done'; at?: string }
	| { kind: 'building'; at: string }
	| { kind: 'planned'; at: string }
	| { kind: 'overview' }
	| { kind: 'design' };

export interface DocPage {
	/** The page's address inside the docs, with a trailing slash (the base path is added on output). */
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
	/** A short name for the section's tab in the header and its card on the home page. */
	short: string;
	pages: DocPage[];
}

export const done = (at?: string): Capability => (at === undefined ? { kind: 'done' } : { kind: 'done', at });
export const building = (at: string): Capability => ({ kind: 'building', at });
export const planned = (at: string): Capability => ({ kind: 'planned', at });
const overview: Capability = { kind: 'overview' };

function md(href: string, title: string, description: string, capability: Capability): DocPage {
	return { href, title, description, capability, source: `apps/docs/src/routes${href}+page.md` };
}

function svelte(href: string, title: string, description: string, capability: Capability, source?: string): DocPage {
	return { href, title, description, capability, source: source ?? `apps/docs/src/routes${href}+page.svelte` };
}

export const HOME = svelte(
	'/',
	'Colophon documentation',
	'Guides, concepts and references for Colophon CMS, an open-source Rust and Svelte CMS that can prove what it published.',
	overview
);

export const SECTIONS: Section[] = [
	{
		title: 'Get started',
		short: 'Start',
		pages: [
			md('/get-started/', 'What Colophon is', 'A CMS that can prove what it published, built in the open; pre-release, v0.1 is not out.', overview),
			md('/get-started/quickstart/', 'Run it locally', 'The local stack, the migrations, the first administrator, the dev users, the server, the Studio and the CLI.', done('1.8')),
			md('/get-started/first-project/', 'Your first project', 'Create a project as the administrator, invite the people who will write and publish, and see what each role may do.', done('1.8')),
			md('/get-started/first-article/', 'Your first article', 'Write an article in the Studio or from the command line, and watch every save become a version.', done('1.6')),
			md('/get-started/first-publish/', 'Your first publish and receipt', 'Ask for review, approve, publish, and read the receipt that says where the release was seen.', done('3.3'))
		]
	},
	{
		title: 'For editors',
		short: 'Editors',
		pages: [
			md('/editors/', 'Editor guide', 'Writing, review and publishing in the Studio, in plain words.', overview),
			md('/editors/joining/', 'Joining a project', 'Signing in, accepting an invitation link, and what your role lets you do.', done('1.7')),
			md('/editors/writing/', 'Writing and saving', 'How the Studio saves your work, what its save messages mean, and what happens when two people edit.', done('1.3')),
			md('/editors/versions/', 'Versions and comparing', 'Every save is a version you can look back at and compare with your draft.', done('3.5')),
			md('/editors/url-names/', 'URL names', 'The part of a web address that names your article: making one from the title with one button, the rules, and why two articles can’t share one.', done('3.6')),
			md('/editors/review/', 'Publish requests and review', 'Gather what you want to publish into a publish request, ask for review, and what makes an approval run out.', done('2.1')),
			md('/editors/publishing/', 'Publishing', 'Publishing an approved request, and what happens when someone else published first.', done('2.2')),
			md('/editors/receipts/', 'Receipts: is it on the website?', '"Published in CMS", "Website update pending", "Verified at 1 location" and "Needs attention", and what to do about each.', done('3.3')),
			md('/editors/taking-down/', 'Taking something down', 'Taking an article off the website, what can and can’t be taken back, and what happens to old links.', done('3.2'))
		]
	},
	{
		title: 'Concepts',
		short: 'Concepts',
		pages: [
			md('/concepts/', 'Concepts', 'How Colophon keeps its promises, one idea at a time.', overview),
			md('/concepts/drafts-and-versions/', 'Drafts and versions', 'Every save is a new version that never changes; the draft points at the latest.', done('1.2')),
			md('/concepts/saves-and-conflicts/', 'Saves and conflicts', 'A save names the version it builds on, so a stale save is refused, never merged silently.', done('1.3')),
			md('/concepts/content-types/', 'Content types and URL names', 'Versioned definitions, a builder whose changes are checked against stored content, and URL names unique per collection.', done('3.6')),
			md('/concepts/roles-and-permissions/', 'Roles and permissions', 'Four default roles, roles owners define, permissions narrowed to fields or to your own entries, and one deny-by-default evaluator.', done('R.3')),
			md('/concepts/publish-requests/', 'Publish requests', 'A publish request (changeset) pins exact versions, and an approval counts only for those versions.', done('2.1')),
			md('/concepts/releases-and-generations/', 'Releases and generations', 'A publish is one transaction that takes the next generation; generations only move forward.', done('2.2')),
			md('/concepts/outbox-and-worker/', 'The outbox and the worker', 'External work is committed as jobs with the release, then done by a leased worker with a stable key.', done('2.3')),
			md('/concepts/caching-and-delivery/', 'Caching and delivery', 'Public answers cached under keys that name their generation, which always comes from PostgreSQL.', done('3.1')),
			md('/concepts/unpublish-and-revocation/', 'Unpublish and revocation', 'Tombstones, revocation through historical URLs, and CDN purges that report what they could not prove.', done('3.2')),
			md('/concepts/receipts-and-probes/', 'Receipts and probes', 'Each release gets one label, and "verified" always says at how many locations.', done('3.3')),
			md('/concepts/scheduled-publishing/', 'Scheduled publishing', 'Publishing at a time you choose, rechecked when it is due, and labelled delayed when it runs late.', done('D-060')),
			md('/concepts/onboarding/', 'Onboarding and plain words', 'A first-run setup code, a checklist, one-click publishing and one glossary of plain words for people who aren’t developers.', done('3.7')),
			md('/concepts/simulator/', 'The deterministic simulator', 'Thousands of seeded runs with crashes and network faults; a failure is a seed you can replay.', done('2.4'))
		]
	},
	{
		title: 'Developer reference',
		short: 'Reference',
		pages: [
			md('/reference/', 'Reference', 'The HTTP APIs, the command line, the rich-text format and webhooks.', overview),
			md('/reference/api/', 'API conventions', 'Authentication, response shapes, If-Match and ETags, idempotency keys, Colophon-Generation, pagination and limits.', done('1.2')),
			md('/reference/api/routes/', 'All routes', 'Every route the server answers, with its method, who may call it and where it is documented.', overview),
			md('/reference/api/auth/', 'Sign-in and sessions', 'Health, OpenID Connect sign-in, sessions, device sign-in for the CLI, and /me.', done('1.6')),
			md('/reference/api/projects/', 'Projects, members and invitations', 'Creating projects, listing and changing members, and single-use invitation links.', done('1.8')),
			md('/reference/api/roles/', 'Roles', 'Listing the roles in a project, and creating, changing and deleting the ones owners define, down to fields and own entries.', done('R.3')),
			md('/reference/api/content-types/', 'Content types', 'Content type definitions, their field types, versions and the changes allowed against stored content.', done('3.6')),
			md('/reference/api/entries/', 'Entries and versions', 'Creating, listing, searching and saving entries, their version history, and rich-text validation.', done('3.5')),
			md('/reference/api/publish-requests/', 'Publish requests and reviews', 'Changesets: creating, submitting, refreshing, reviewing and publishing them.', done('2.2')),
			md('/reference/api/releases/', 'Releases and receipts', 'The release list, the latest generation, and each release’s receipt with every observation.', done('3.3')),
			md('/reference/api/website-connections/', 'Website connections and jobs', 'Destinations (webhooks, CDN purges and probes) and the worker’s jobs for a release.', done('3.3')),
			md('/reference/api/errors/', 'Errors', 'The error envelope, every error code and every reason in details.', done('0.3')),
			md('/reference/delivery-api/', 'Delivery API', 'The public, read-only /content/v1 API: latest and pinned reads, ETags, cache headers and 410.', done('3.2')),
			md('/reference/cli/', 'Command line', 'The colophon command: sign-in, content, the operator commands, JSON output and exit codes.', done('1.8')),
			md('/reference/rich-text/', 'Rich-text format', 'colophon/rich-text v1: the stored JSON of every rich-text field, its limits and error codes.', done('0.5')),
			md('/reference/webhooks/', 'Webhooks', 'The release.committed payload, CDN purges and probes: headers, retries, addresses and what a receiver must do.', done('2.3'))
		]
	},
	{
		title: 'Operations',
		short: 'Operations',
		pages: [
			md('/operations/', 'Operations', 'Running Colophon: what it needs, how to configure it, and how to keep it healthy.', overview),
			md('/operations/requirements/', 'Self-hosting requirements', 'What Colophon needs to run, and what is not ready for production yet.', overview),
			md('/operations/local-stack/', 'The local stack', 'PostgreSQL, Valkey, Garage and Keycloak on 127.0.0.1, and the checks that prove each works.', done('0.2')),
			md('/operations/configuration/', 'Configuration', 'Every environment variable the server, the worker, the CLI, the scripts and the tests read, with its default.', done('3.3')),
			md('/operations/health-checks/', 'Health checks', '/livez, /readyz and colophon doctor: what each checks and what each answer means.', done('0.3')),
			md('/operations/worker/', 'The worker', 'Running the worker, its leases, retries, locations and the jobs it does.', done('3.3')),
			md('/operations/upgrades/', 'Upgrades and migrations', 'Every database migration, what it does, and how to apply them.', done('3.5')),
			md('/operations/backups/', 'Backups and restore', 'What the backup and restore runbooks will cover, and why none of it exists yet.', planned('P6')),
			md('/operations/crash-tests/', 'Crash tests', 'Fault points, the real-process crash loop and the simulator sweeps that CI runs.', done('2.4'))
		]
	},
	{
		title: 'Resources',
		short: 'Resources',
		pages: [
			md('/resources/security/', 'Security model', 'Who can do what, how projects are isolated, how sign-in works, and what is not protected yet.', done('1.8')),
			svelte('/resources/performance/', 'Performance', 'Every load result kept in perf/results, with the machine and date it ran on; never a claim about production.', done('3.5')),
			md('/resources/roadmap/', 'Roadmap and status', 'Every milestone and step: what is done, what is being built, and what is planned.', overview),
			md('/resources/faq/', 'FAQ', 'Short answers to the questions people ask first.', overview),
			md('/resources/glossary/', 'Glossary', 'The Studio’s plain words and the API’s terms, side by side.', overview)
		]
	},
	{
		title: 'Updates',
		short: 'Updates',
		pages: [
			svelte('/changelog/', 'Changelog', 'Every step merged into main, newest first, and what is being built on branches; from CHANGELOG.md.', overview, 'CHANGELOG.md'),
			svelte('/releases/', 'Release notes', 'One page per milestone: what it lets you do, its evidence and its known limits.', overview),
			svelte('/upgrading/', 'Upgrade guides', 'One guide per range of database migrations: what changes, the steps, and how to check it worked.', overview)
		]
	}
];

/** The design documents' section; its pages come from the repository (see the root layout). */
export const DESIGN = { title: 'Design documents', href: '/design/' } as const;

export const PAGES: DocPage[] = SECTIONS.flatMap((section) => section.pages);

/** The page at an address inside the docs (without the base path), if the site map has it. */
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
