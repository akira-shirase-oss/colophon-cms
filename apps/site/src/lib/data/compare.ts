// Colophon, Strapi and Sanity, transcribed from docs/COMPARISON.md as it stood on 2026-10-27 (on
// the step 3.6 branch). Keep the source's evidence notes with it: what was checked against the
// vendors' docs, and what is general knowledge. Backticks mark code.
import type { StatusKind } from '#lib/status.ts';

export const EVIDENCE = {
	checked: '2026-10-27',
	strapi:
		'a local Strapi 5.59.0 project (its schema files and installed packages) and docs.strapi.io',
	sanity: 'sanity.io/docs only; there is no local Sanity project',
	note: 'Pricing and plan tiers are the vendors’ own claims and change often. Where a cell says “not checked”, nobody has looked.',
	backlog:
		'Only the backlog rows also covered by the tables above were checked against the vendors’ docs (2026-10-27); the rest, and every plan tier, are from general knowledge and get checked by the step that builds them.',
	reconciled:
		'Some cells of the source’s tables predate D-057, which moved the modelling features into milestone M and live collaboration and GraphQL from non-goals into milestones E and D. Here those cells follow D-057 and the backlog: the “Take” column on modelling, live co-editing and the query language.'
};

export interface CompareTable {
	id: string;
	title: string;
	lead?: string;
	columns: string[];
	rows: string[][];
}

export const TABLES: CompareTable[] = [
	{
		id: 'modelling',
		title: 'Content modelling',
		columns: ['', 'Strapi 5', 'Sanity', 'Colophon', 'Take'],
		rows: [
			[
				'Field types',
				'Text (short, long), rich text (Blocks or Markdown), number (integer, big integer, decimal, float), date/datetime/time, boolean, enumeration, email, password, JSON, UID, media, relation (six kinds), component (single or repeatable), dynamic zone, marketplace custom fields',
				'string, text, number, boolean, date, datetime, url, email, slug, image, file, reference, cross-dataset and global references, array, object, block (Portable Text), geopoint',
				'text, rich text, number, boolean, enum, datetime (D-040); image with media (P4); reference and bounded list planned',
				'Slug in step 3.6; image with media in P4; references, lists and the other field types in milestone M (see the backlog). Password: not adopted'
			],
			[
				'Groups of fields',
				'Components: reusable, single or repeatable, not content on their own',
				'Object types, nestable, arrays of objects',
				'None',
				'Milestone M (D-057): components, nested and repeatable, as bounded lists of objects'
			],
			[
				'Page builder',
				'Dynamic zones: an ordered list of components',
				'Arrays of object types',
				'Rich text has its own validated node set',
				'Milestone M (D-057): block lists, a format change that ships with new fixtures'
			],
			[
				'One-of-a-kind content',
				'Single types',
				'Documents with a fixed id, arranged in the Structure tool',
				'Collections only',
				'Milestone M (D-057): single types'
			],
			[
				'Validation',
				'required, unique, min/max length or value, regex, default value',
				'Rule chains (`required`, `either`, `all`), `custom()` functions in the Studio',
				'required, max characters, min/max, integer; server-side, with a code per failure',
				'Pattern, uniqueness, length and value in milestone M, uniqueness enforced by the database (prechecks alone race). No custom-code validators: the server runs no user code'
			],
			[
				'Conditional fields',
				'Shown or hidden by a boolean or enumeration field',
				'`hidden` and `readOnly` callbacks',
				'None',
				'Milestone M (D-057)'
			],
			[
				'AI schema design',
				'Strapi AI (Growth plan) builds content types from prompts, code, Figma or an image',
				'Not checked',
				'Agents propose drafts only (P5)',
				'Not adopted. A schema change is the riskiest edit a CMS has; it stays a reviewed human action'
			]
		]
	},
	{
		id: 'schema-changes',
		title: 'Changing a schema that already has content',
		lead: 'Where the products differ most. Neither Strapi nor Sanity tells an editor, before a change, which stored entries it would break; Colophon refuses any change that would invalidate a stored version, and step 3.6 makes the refusal name them.',
		columns: ['', 'Strapi 5', 'Sanity', 'Colophon'],
		rows: [
			[
				'Where the schema lives',
				'JSON files in the project, written by the Content-Type Builder',
				'JavaScript/TypeScript in the Studio’s code',
				'Versioned definitions in PostgreSQL, changed through the API (D-040)'
			],
			[
				'Who can change it, where',
				'The builder is “available in Development environment only”; changes reach production as a code deploy',
				'Developers, by deploying Studio code',
				'Owners and developers, at runtime, through the same API as everything else'
			],
			[
				'Does the store enforce it',
				'Yes, through database columns',
				'No. “Changing the schema … will not automatically change or delete existing content”',
				'Yes. Every revision records the definition version it was validated against'
			],
			[
				'Renaming a field',
				'Since 5.57.0 the column is renamed and data kept; renaming and changing its type recreates the field empty',
				'Old data stays under the old name until a migration script moves it',
				'Refused with `needs_migration` today; migrations with plans and approvals come later'
			],
			['Removing a type', 'Entries vanish from the admin; data stays in the database', 'Documents stay in the dataset', 'Not possible yet']
		]
	},
	{
		id: 'editing',
		title: 'Editing, history and review',
		columns: ['', 'Strapi 5', 'Sanity', 'Colophon'],
		rows: [
			[
				'Drafts',
				'Draft & Publish, per content type',
				'One draft per document; later edits update it',
				'Insert-only revisions, saves with `If-Match`, autosave, a device copy and a conflict screen (P1)'
			],
			[
				'Live co-editing',
				'Not checked',
				'Real-time, with presence',
				'Not a goal in the original spec (no CRDT co-editing); since D-057, live collaboration is planned for milestone E, design open (O-017). Conflicts are detected and shown, never merged silently'
			],
			[
				'History',
				'Content History, paid (Growth, Enterprise): 14 days on Growth, 30 to 90 on Enterprise, and only for edits made in the admin panel. REST, GraphQL, Document Service and import changes are not versioned',
				'Not checked',
				'Every revision from every path (Studio, CLI, API) is kept for good; the release slider and pinned pages are P4'
			],
			[
				'Review',
				'Review Workflows (the package is installed in the demo; plan tier not checked)',
				'Not checked',
				'Changesets pin exact revisions; a review approves one hash; no self-approval (2.1)'
			]
		]
	},
	{
		id: 'publishing',
		title: 'Publishing and delivery',
		columns: ['', 'Strapi 5', 'Sanity', 'Colophon'],
		rows: [
			[
				'Publishing several entries together',
				'Content Releases (the package is installed in the demo)',
				'Content Releases, on some Enterprise plans: at most 1,000 documents and 100 MB a release; Scheduled Drafts for one document on Growth',
				'A release is one transaction on the publisher’s base generation; generations only move forward (2.2)'
			],
			[
				'Telling other systems',
				'Webhooks',
				'GROQ-powered webhooks, which per Sanity’s docs don’t yet support the API version that release versions need',
				'A transactional outbox and a leased worker, a stable idempotency key, public addresses only (2.3)'
			],
			[
				'Knowing it reached the site',
				'Not offered',
				'Not offered',
				'Probes look for the release’s generation marker; receipts say verified at N locations, or pending (3.3)'
			],
			[
				'Taking content back',
				'Unpublish',
				'Unpublish',
				'Tombstones, origin revocation through historical URLs, CDN purge jobs, remaining exposure shown (3.2)'
			],
			[
				'Reading drafts and releases',
				'Preview',
				'Perspectives: `drafts`, `published`, or a stack of releases',
				'Reads pinned to a generation (2.2); preview links are v0.1 core'
			],
			[
				'Query language',
				'REST, GraphQL plugin',
				'GROQ, GraphQL',
				'REST (`/content/v1`) today; a query API, GraphQL and a TypeScript SDK in milestone D'
			]
		]
	}
];

export const DIFFERENT_ON_PURPOSE: { title: string; text: string }[] = [
	{
		title: 'No server plugins',
		text: 'Arbitrary untrusted native plugins stay out (D-057): code from strangers running in the server would break the invariants every other feature relies on. Milestone D offers the same reach safely: custom field types declared as data, webhooks by event, and Studio panels in sandboxed iframes.'
	},
	{
		title: 'Schema changes are not code deploys',
		text: 'Content types stay data, versioned in PostgreSQL, so the same authorization, history and refusal rules apply to them as to content.'
	},
	{
		title: 'Flexibility arrives with its proof',
		text: 'Dynamic zones, nested objects and conditional fields are in milestone M; each ships with the validator, renderer and simulator coverage it needs, not before.'
	}
];

export interface ParityRow {
	feature: string;
	strapi: string;
	sanity: string;
	colophon: string;
	kind: StatusKind;
}

const row = (feature: string, strapi: string, sanity: string, colophon: string, kind: StatusKind): ParityRow => ({
	feature,
	strapi,
	sanity,
	colophon,
	kind
});

export const PARITY: ParityRow[] = [
	row('Drafts with autosave, conflict detection', '✓', '✓', 'done (P1)', 'done'),
	row('Full history of every revision, from every write path', 'Paid, admin edits only', '✓', 'done (P1); history UI and restore in P4', 'done'),
	row('Content types defined at runtime, safe changes', 'Dev-only builder', 'Code', 'done (D-040); builder and data-aware changes building (3.6)', 'done'),
	row('Text, rich text, number, boolean, enum, date-time', '✓', '✓', 'done (D-040)', 'done'),
	row('Slug / UID, unique per collection', '✓', '✓', 'building (3.6)', 'building'),
	row('References / relations (one, many, two-way)', '✓', '✓', 'M', 'planned'),
	row('Components / objects, repeatable lists', '✓', '✓', 'M', 'planned'),
	row('Dynamic zones / block arrays (page builder)', '✓', '✓', 'M', 'planned'),
	row('Single types / singletons', '✓', '✓', 'M', 'planned'),
	row('Email, URL, date, time, JSON, Markdown, colour, geopoint fields', '✓', '✓', 'M', 'planned'),
	row('Validation: pattern, unique, length, value; conditional fields; defaults', '✓', '✓', 'M', 'planned'),
	row('Field descriptions, groups and tabs, custom previews', '✓', '✓', 'M', 'planned'),
	row('Schema migrations with a plan', 'Rename only', 'Scripts', 'M', 'planned'),
	row('Media library: uploads, folders, alt text, image transforms, hotspot and crop', '✓', '✓', 'P4', 'planned'),
	row('Review before publish', 'Review workflows (paid)', 'Tasks, comments', 'done: changesets and hash-bound reviews (P2); stages in E', 'done'),
	row('Publish several entries together (releases)', '✓', '✓ (Enterprise)', 'done (P2)', 'done'),
	row('Proof that a release reached the site', '–', '–', 'done (P3)', 'done'),
	row('Unpublish with revocation', 'Unpublish', 'Unpublish', 'done (P3)', 'done'),
	row('Scheduled publishing', '✓', '✓', 'E', 'planned'),
	row('Localization (i18n)', '✓', 'Plugins', 'E (data model ready since D-034)', 'planned'),
	row('Comments and tasks', '–', '✓', 'E', 'planned'),
	row('Real-time collaboration and presence', '–', '✓', 'E (O-017)', 'planned'),
	row('Visual preview / click-to-edit', 'Preview', 'Presentation', 'E', 'planned'),
	row('Bulk actions, duplicate, saved views', '✓', '✓', 'E', 'planned'),
	row('Roles: built-in and custom', '✓', '✓ (by plan)', 'R (four fixed roles done, D-042)', 'planned'),
	row('Field-level and “own entries” permissions', '✓', '✓ (by plan)', 'R', 'planned'),
	row('API tokens: read-only, full, custom', '✓', '✓', 'R (device sign-in and bearer tokens done, D-041)', 'planned'),
	row('Audit log', 'Paid', '✓ (by plan)', 'R', 'planned'),
	row('SSO / OpenID Connect', 'Paid', '✓ (by plan)', 'done (Keycloak, D-031)', 'done'),
	row('REST delivery with caching', '✓', '✓', 'done (P2, P3)', 'done'),
	row('Query API: filters, sort, populate, field selection, perspectives', '✓', 'GROQ', 'D', 'planned'),
	row('GraphQL', 'Plugin', '✓', 'D', 'planned'),
	row('Webhooks', '✓', '✓', 'done (P2); by event in D', 'done'),
	row('TypeScript SDK and generated types', '✓', '✓', 'D', 'planned'),
	row('Import and export, from Strapi and Sanity', 'Transfer', 'CLI', 'D', 'planned'),
	row('CLI', '✓', '✓', 'done (P1); full contract in P5', 'done'),
	row('Extension points (custom fields, Studio panels)', 'Plugins', 'Plugins', 'D, sandboxed (D-057)', 'planned'),
	row('AI assistance', 'Strapi AI (paid)', 'AI Assist', 'P5, agents propose only', 'planned')
];

/** The rows the home page shows, by feature name. */
export const TEASER = [
	'Proof that a release reached the site',
	'Full history of every revision, from every write path',
	'Publish several entries together (releases)',
	'Roles: built-in and custom',
	'GraphQL'
];

export const CONVEX: { idea: string; colophon: string; when: string }[] = [
	{
		idea: 'Reactive queries: a write reruns the queries that read it',
		colophon: 'A live Studio: lists, drafts and receipts update as PostgreSQL commits; a hint says what changed and the client re-reads through the API',
		when: 'E'
	},
	{
		idea: 'A cache that is always consistent',
		colophon: 'Already the delivery cache’s rule (keys name the release, read from PostgreSQL each time); extended to Studio and draft reads',
		when: 'D'
	},
	{ idea: 'One snapshot per client', colophon: 'Studio pages read at one release and draft version', when: 'E' },
	{ idea: 'Side effects outside transactions', colophon: 'Already invariant 2: the outbox and the worker', when: 'done (P2)' },
	{ idea: 'Deterministic functions', colophon: 'Already how the simulator works (D-039)', when: 'done (P1)' },
	{ idea: 'Indexes declared in the schema', colophon: 'The query API filters and sorts only on fields a content type declares as indexed', when: 'D' }
];
