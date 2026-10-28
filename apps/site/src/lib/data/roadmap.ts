// The milestones, in build order, from docs/ROADMAP.md: the numbered ones, the parity milestones R,
// M, E and D (D-057), and S and V (D-062). Estimates are the roadmap's own guesses at ~45 hours a
// week, not measurements. Statuses from what is merged (CHANGELOG.md): a milestone is done only
// when its proof gate passes, so one with some steps done is still under way.
import type { StatusKind } from '#lib/status.ts';

export interface Step {
	id: string;
	title: string;
	kind: StatusKind;
	/** What the status leaves out: for a step in progress, what is happening to it; for a done
	 * step, what is still to check. */
	note?: string;
}

export interface Milestone {
	id: string;
	name: string;
	goal: string;
	gate: string;
	estimate: string;
	kind: StatusKind;
	/** Added after the status's word, when it says more: "October 2026", "v0.1". */
	label: string;
	steps?: Step[];
	/** For a milestone without numbered steps: what it has built so far, ahead of the rest. */
	built?: string[];
	bullets?: string[];
}

export const TOTAL_ESTIMATE = 'about 10–13 months to v0.1 at the pace the estimates assumed, with the first public demo at the end of P3';

export const MILESTONES: Milestone[] = [
	{
		id: 'P0',
		name: 'Foundation',
		goal: 'A repository where every later change lands on pinned, tested ground.',
		gate: 'Editor JSON round-trips; Rust rejects invalid documents; CI is green from a clean clone.',
		estimate: '1–2 weeks',
		kind: 'done',
		label: 'October 2026',
		steps: [
			{ id: '0.1', title: 'Pinned toolchain, workspace and CI', kind: 'done' },
			{ id: '0.2', title: 'The local stack', kind: 'done' },
			{ id: '0.3', title: 'Errors, honest health, the testbed and load tests', kind: 'done' },
			{ id: '0.4', title: 'The rich-text format and the editor', kind: 'done' },
			{ id: '0.5', title: 'The Rust validator', kind: 'done' },
			{ id: '0.6', title: 'The renderer', kind: 'done' },
			{ id: '0.7', title: 'The Studio’s first screen', kind: 'done' }
		]
	},
	{
		id: 'P1',
		name: 'Never lose an edit',
		goal: 'An editor and the command line create the same validated draft; conflicts are safe; projects are isolated.',
		gate: 'Killed before or after a commit, a save is neither falsely acknowledged nor stored twice (F01, F02); two editors on one version: one is refused, nothing lost (F04); another project is denied.',
		estimate: '3–5 weeks',
		kind: 'done',
		label: '',
		steps: [
			{ id: '1.1', title: 'Signing in, and projects that can’t see each other', kind: 'done' },
			{ id: '1.2', title: 'A draft that can’t be lost', kind: 'done' },
			{ id: '1.3', title: 'Two editors, one article', kind: 'done' },
			{ id: '1.4', title: 'The first simulator run', kind: 'done' },
			{ id: '1.5', title: 'Content types you define', kind: 'done' },
			{ id: '1.6', title: 'The command line saves the same draft', kind: 'done' },
			{ id: '1.7', title: 'Inviting people', kind: 'done' },
			{ id: '1.8', title: 'The first owner', kind: 'done' }
		]
	},
	{
		id: 'P2',
		name: 'Publish exactly once',
		goal: 'Reviewed changes become one atomic release, and outside work happens exactly as intended despite crashes.',
		gate: 'A worker killed after an external call reconciles without a duplicated effect (F03); an old delivery after a newer release is superseded (F05); two releases on one base can’t both win.',
		estimate: '3–4 weeks',
		kind: 'done',
		label: '',
		steps: [
			{ id: '2.1', title: 'Approving the exact change', kind: 'done' },
			{ id: '2.2', title: 'One release, one number', kind: 'done' },
			{ id: '2.3', title: 'Work that survives a crash', kind: 'done' },
			{ id: '2.4', title: 'Killing it on purpose', kind: 'done' }
		]
	},
	{
		id: 'P3',
		name: 'Prove it reached the site',
		goal: 'Public reads are cached safely, invalidated correctly, and every release has evidence.',
		gate: 'A new article invalidates listings (F06); unpublished content is denied at the origin (F07); losing the cache loses nothing (F08); the mixed-release checker passes. The first public demo: a recorded crash loop and a live site with receipts.',
		estimate: '3–4 weeks',
		kind: 'building',
		label: '',
		steps: [
			{ id: '3.1', title: 'A cache that can’t serve the wrong thing', kind: 'done' },
			{ id: '3.2', title: 'Taking it back', kind: 'done' },
			{ id: '3.3', title: 'A receipt for every publish', kind: 'done' },
			{ id: '3.4', title: 'My own site, running on Colophon (locally)', kind: 'done' },
			{ id: '3.5', title: 'The Studio, designed', kind: 'done' },
			{ id: '3.6', title: 'Content modelling in the Studio: slugs and the content-type builder', kind: 'done' },
			{
				id: '3.7',
				title: 'The non-technical path: plain words, one-click publish, a first-run checklist, sign-up with a setup code',
				kind: 'done',
				note: 'not yet checked at phone width'
			},
			{ id: '3.8', title: 'The site goes live, and the first public demo', kind: 'planned', note: 'next' }
		]
	},
	{
		id: 'R',
		name: 'Roles and permissions',
		goal: 'Owners decide exactly who may do what, down to a field, and every path enforces it the same way (D-058).',
		gate: 'Every action, role, field and condition decided by one evaluator, exhaustive for the built-in roles and property-tested for random ones; a denial test on every route; a token can do exactly what its permissions allow; a load scenario with custom roles shows no regression.',
		estimate: '2–3 weeks',
		kind: 'building',
		label: '',
		steps: [
			{ id: 'R.1', title: 'Roles as data, behaviour unchanged', kind: 'done' },
			{ id: 'R.2', title: 'Custom roles and the Studio’s role editor', kind: 'done' },
			{ id: 'R.3', title: 'Field-level permissions and “only their own”', kind: 'done' },
			{ id: 'R.4', title: 'API tokens and the audit log', kind: 'planned', note: 'next' }
		]
	},
	{
		id: 'M',
		name: 'Content modelling parity',
		goal: 'The shapes Strapi and Sanity model are content types here, validated in Rust and kept valid as they change.',
		gate: 'Relations, components, lists, blocks and single types round-trip byte for byte and stay valid through schema changes; references never dangle; a migration that fails halfway resumes without duplicates.',
		estimate: '4–6 weeks',
		kind: 'planned',
		label: '',
		bullets: [
			'References, components, bounded lists, page-builder blocks, single types',
			'Email, URL, date, time, JSON, Markdown and colour fields; descriptions, defaults, groups and tabs',
			'Validation by pattern, uniqueness, length and value; conditional fields',
			'Migrations with a plan, a dry run and approval'
		]
	},
	{
		id: 'P4',
		name: 'Time travel and media',
		goal: 'Any release can be viewed and rolled back; uploads are safe.',
		gate: 'Rollback and pinned-read tests; revoked content stays unavailable through historical releases; malicious uploads are rejected; a large media workload keeps editing responsive.',
		estimate: '3–4 weeks',
		kind: 'planned',
		label: '',
		bullets: [
			'The release slider, history and compare, restore as a new version',
			'The media library: uploads checked in quarantine, derivatives, cleanup',
			'Preview links on a separate origin; sitemaps and feeds from one release'
		]
	},
	{
		id: 'E',
		name: 'Editorial parity',
		goal: 'Everything an editorial team does in Strapi or Sanity, with nothing lost and every change attributed.',
		gate: 'Two editors at once never lose an acknowledged edit; a schedule missed during downtime is reported as delayed, never on time; locale variants publish independently; comments and tasks follow permissions.',
		estimate: '5–7 weeks',
		kind: 'building',
		label: '',
		built: ['Scheduled publishing in a time zone, with daylight-saving edges chosen, never guessed, built first (D-060)'],
		bullets: [
			'Localization: locale variants with their own URL names, drafts and history',
			'Review stages, assignees, comments and tasks',
			'Live presence and collaboration; a live Studio that updates as PostgreSQL commits',
			'Visual preview with click-to-edit'
		]
	},
	{
		id: 'D',
		name: 'Developer parity',
		goal: 'Developers get what Strapi’s and Sanity’s APIs give them, through the same authorization.',
		gate: 'Every query is permission-filtered; GraphQL and REST answer the same; the Strapi and Sanity reference datasets import and round-trip.',
		estimate: '4–5 weeks',
		kind: 'planned',
		label: '',
		bullets: [
			'A query API with filters, sorting, field selection and perspectives',
			'GraphQL, OpenAPI, and the TypeScript SDK with generated types',
			'Webhooks by event; import from Strapi and Sanity; export',
			'Extension points instead of plugins: custom field types as data, sandboxed Studio panels'
		]
	},
	{
		id: 'S',
		name: 'Scale and performance',
		goal: 'Colophon runs as several servers and workers behind a load balancer, and every scenario has a known breaking point and fails safely past it (D-062).',
		gate: 'A server killed under load behind the balancer fails only its in-flight requests, and idempotent retries succeed; a draining server finishes what it accepted; rate limits hold across instances and when Valkey is down; every scenario has a stored stress result; three servers are measured against one.',
		estimate: '2–3 weeks',
		kind: 'planned',
		label: '',
		bullets: [
			'A local multi-instance stack: HAProxy in front of three servers and two workers',
			'Rate limits shared across instances, readiness that drains, a connection budget summed across them',
			'A stress run for every load scenario, on one, two and three servers',
			'Rust benchmarks for the hot paths, with a regression check in CI'
		]
	},
	{
		id: 'V',
		name: 'Versions and distribution',
		goal: 'Anyone can install the command line, the SDK and the MCP server without building Rust, and each says whether it fits the server it talks to (D-062).',
		gate: 'A release dry run builds every artifact; npm, Cargo, Homebrew and an install script each install the same version on a clean machine in CI; an old CLI against a new server, and the reverse, gets a plain-words answer.',
		estimate: '1–2 weeks',
		kind: 'planned',
		label: '',
		bullets: [
			'One version for the server, worker, CLI, SDK and MCP server; /api/v1 is the contract',
			'Binaries for macOS, Linux and Windows with checksums and signed provenance',
			'The CLI on npm as @colophon-cms/cli, with cargo install, a Homebrew tap and an install script',
			'Container images for the server and worker'
		]
	},
	{
		id: 'P5',
		name: 'Safe automation',
		goal: 'Scripts and agents do permitted work at bounded scale and never execute an unauthorized action.',
		gate: 'Zero unauthorized actions across the evaluation set; an agent interrupted halfway through a bulk edit resumes without duplicates; a noisy agent is throttled while editors keep their latency.',
		estimate: '3–4 weeks',
		kind: 'planned',
		label: '',
		bullets: [
			'Machine credentials, budgets reserved in PostgreSQL, a kill switch',
			'Bulk jobs with per-item outcomes; the MCP adapter',
			'The full command-line JSON contract; import and export',
			'Optionally, an AI writing assistant, off by default'
		]
	},
	{
		id: 'P6',
		name: 'Operate it',
		goal: 'Someone other than the maintainer can install, upgrade, back up, restore and diagnose Colophon.',
		gate: 'The full fault and load matrix, a 24-hour soak, a restore drill on clean infrastructure, five editors through the usability test, one independent install. Then v0.1.',
		estimate: '3–4 weeks',
		kind: 'planned',
		label: 'v0.1',
		bullets: [
			'Encrypted backups, WAL archiving and the restore procedure',
			'The benchmark report, complete docs, signed artifacts and an SBOM',
			'A usability test with five editors and one independent install'
		]
	}
];
