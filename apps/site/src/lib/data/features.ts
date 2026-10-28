// The product, feature by feature, grouped like the parity backlog in docs/COMPARISON.md and
// labelled from it, from docs/ROADMAP.md and from what is merged (CHANGELOG.md). Checked
// 2026-10-27, after steps 3.6, 3.7, R.1 to R.3 and scheduled publishing merged.
import { done, planned, type Status } from '#lib/status.ts';

export interface Feature {
	name: string;
	detail: string;
	status: Status;
}

export interface FeatureGroup {
	id: string;
	title: string;
	lead: string;
	features: Feature[];
}

export const FEATURE_GROUPS: FeatureGroup[] = [
	{
		id: 'writing',
		title: 'Writing and editing',
		lead: 'Drafts you can’t lose, and two people on one article without anyone overwriting anyone.',
		features: [
			{
				name: 'Drafts that can’t be lost',
				detail:
					'Autosave, a copy of unsaved edits on the device, and saves that name the version they were made on. A save on a stale version is refused, never merged silently.',
				status: done('P1')
			},
			{
				name: 'Two editors, one article',
				detail: 'A conflict screen shows both versions and lets the editor choose.',
				status: done('P1')
			},
			{
				name: 'Every version kept, from every path',
				detail:
					'Studio, command line and API writes all become insert-only versions. Browsing and restoring them in the Studio comes with time travel (P4).',
				status: done('P1')
			},
			{
				name: 'Rich text that is checked, not trusted',
				detail:
					'Stored as validated JSON, never HTML. The editor opens a document only if it can give it back byte for byte, and the server refuses anything outside the format.',
				status: done('P0')
			},
			{
				name: 'Title search, entry states, version comparison',
				detail: 'Find entries by title, see draft, live or changed at a glance, compare versions, and review exact diffs.',
				status: done('P3')
			},
			{
				name: 'Plain words and one obvious path',
				detail:
					'One glossary for every screen, one-click publish, a first-run checklist, empty states that teach, and technical detail only on request (D-059). Not yet checked at phone width.',
				status: done('3.7')
			},
			{ name: 'Localization', detail: 'Locale variants with their own URL names, drafts and history.', status: planned('E') },
			{ name: 'Comments and tasks', detail: 'On entries and fields, following permissions.', status: planned('E') },
			{
				name: 'Live presence and collaboration',
				detail: 'See who else is editing; the design is open (O-017), and no acknowledged edit may ever be lost.',
				status: planned('E')
			},
			{ name: 'Bulk actions, duplicate, saved views', detail: 'For editorial teams with many entries.', status: planned('E') }
		]
	},
	{
		id: 'modelling',
		title: 'Content modelling',
		lead: 'Content types are data, versioned in PostgreSQL, and a change is checked against what is already stored.',
		features: [
			{
				name: 'Content types you define',
				detail:
					'Text, rich text, number, yes/no, choice and date-time fields, defined at runtime through the same API as everything else.',
				status: done('P1')
			},
			{
				name: 'Changes that keep stored content valid',
				detail: 'Every version records the definition it was validated against; a change that would break one is refused.',
				status: done('P1')
			},
			{
				name: 'URL names (slugs)',
				detail: 'Unique per collection among drafts and among what is published, enforced by the database; suggested from the title in one click.',
				status: done('3.6')
			},
			{
				name: 'A builder that names what a change would break',
				detail:
					'Add, rename and remove fields in the Studio; a change stored content can’t take is refused, naming the first entries in the way, so an owner can fix them or keep the old shape.',
				status: done('3.6')
			},
			{ name: 'References and relations', detail: 'One and many, with the other side listed; never dangling.', status: planned('M') },
			{ name: 'Components and repeatable lists', detail: 'Reusable groups of fields, nested and repeated.', status: planned('M') },
			{ name: 'Page-builder blocks', detail: 'An ordered list of components, validated like everything else.', status: planned('M') },
			{ name: 'Single types', detail: 'One-of-a-kind content such as a site’s settings or an About page.', status: planned('M') },
			{
				name: 'More field types and validation',
				detail: 'Email, URL, date, time, JSON, Markdown and colour; patterns, uniqueness, defaults and conditional fields.',
				status: planned('M')
			},
			{ name: 'Migrations with a plan', detail: 'A dry run, an approval and a per-entry outcome for the changes refused today.', status: planned('M') }
		]
	},
	{
		id: 'publishing',
		title: 'Review and publishing',
		lead: 'A publish is one numbered release, approved for the exact change, and it isn’t called done until the site shows it.',
		features: [
			{
				name: 'Publish requests for the exact change',
				detail: 'A request pins the exact versions in it; an approval counts only for that set, and any later edit needs approving again.',
				status: done('P2')
			},
			{
				name: 'One release, one number',
				detail: 'Publishing commits in one transaction on the release the publisher saw; two releases on the same base can’t both commit.',
				status: done('P2')
			},
			{
				name: 'Webhooks that survive a crash',
				detail: 'Announced from a transactional outbox by a leased worker, with a stable idempotency key, to public addresses only.',
				status: done('P2')
			},
			{
				name: 'Unpublishing that sticks',
				detail: 'A takedown is refused at every release, historical addresses included, and CDN purges follow it.',
				status: done('P3')
			},
			{
				name: 'Delivery receipts',
				detail: 'Probes look for the release on the live site; each release says “Verified at N locations”, pending, or needs attention.',
				status: done('P3')
			},
			{
				name: 'A static site built from one release',
				detail: 'Every page marked with its release, promoted atomically, never backwards. Runs locally today.',
				status: done('P3')
			},
			{
				name: 'Scheduled publishing',
				detail:
					'An approved request publishes at a time you pick, in a time zone, as you, with every publish check run again; a time the clocks show twice or skip is never guessed, and a late start is labelled delayed (D-060).',
				status: done('D-060')
			},
			{ name: 'Review stages and assignees', detail: 'On top of publish requests.', status: planned('E') },
			{ name: 'Visual preview with click-to-edit', detail: 'The site renders a draft through a short-lived capability.', status: planned('E') }
		]
	},
	{
		id: 'time-travel',
		title: 'Time travel and media',
		lead: 'Every release stays readable; rolling back is just a new release.',
		features: [
			{
				name: 'Reads pinned to any release',
				detail: 'Publication history is stored as non-overlapping intervals, so the API can answer as of any past release.',
				status: done('P2')
			},
			{ name: 'Release slider and rollback', detail: 'Browse the site as it was at release N; roll back as a new release.', status: planned('P4') },
			{ name: 'Media library', detail: 'Uploads checked in quarantine, alt text, image transforms, reference tracking.', status: planned('P4') },
			{ name: 'Preview links', detail: 'Drafts on a separate origin, through expiring capabilities.', status: planned('P4') }
		]
	},
	{
		id: 'access',
		title: 'Roles and access',
		lead: 'Projects are walled off by the database itself; who may do what inside one becomes data you edit.',
		features: [
			{ name: 'Sign-in with OpenID Connect', detail: 'Keycloak as the reference provider; server-side sessions, CSRF and Origin checks.', status: done('P1') },
			{ name: 'Projects that can’t see each other', detail: 'Isolated by PostgreSQL row-level security, not by application checks alone.', status: done('P1') },
			{
				name: 'Four built-in roles',
				detail: 'Owner, developer, editor and publisher, seeded into every project as data and decided by one evaluator (R.1).',
				status: done('P1')
			},
			{ name: 'Invitations', detail: 'Single-use links; a project can never lose its last owner.', status: done('P1') },
			{
				name: 'Setting up from the browser',
				detail:
					'Create an account from the Studio when the identity provider offers sign-up, and make a fresh installation’s first administrator with a one-time code only the server’s terminal shows (D-061).',
				status: done('3.7')
			},
			{
				name: 'Custom roles',
				detail: 'Owners make roles as a grid of plain-word permissions per content type; nobody can hand out more than they hold (D-058).',
				status: done('R.2')
			},
			{
				name: 'Field-level and “only their own” permissions',
				detail: 'A role can be limited to some fields, or to the entries its members created; hidden fields are left out of reads, locked ones refused on writes.',
				status: done('R.3')
			},
			{ name: 'API tokens', detail: 'Read-only, full access or a custom permission set, hashed and expiring.', status: planned('R.4') },
			{ name: 'Audit log', detail: 'Every role, membership and token change, append-only.', status: planned('R.4') }
		]
	},
	{
		id: 'developers',
		title: 'For developers',
		lead: 'One API for the Studio, the command line, scripts and agents, with the same permission checks.',
		features: [
			{ name: 'REST delivery with a safe cache', detail: '/content/v1, cached under keys that name the release, with ETags and a database fallback.', status: done('P3') },
			{ name: 'The colophon command line', detail: 'Sign in with a code approved in the Studio; create and list content; stable exit codes.', status: done('P1') },
			{ name: 'Webhooks by event', detail: 'Release webhooks are done; entry, media and schema events come with the developer milestone.', status: planned('D') },
			{ name: 'Query API', detail: 'Filters, sorting, field selection and reference population, on indexed fields only.', status: planned('D') },
			{ name: 'GraphQL', detail: 'Over the same queries and permissions as REST.', status: planned('D') },
			{ name: 'TypeScript SDK with generated types', detail: 'Types generated from your content types; pinned reads.', status: planned('D') },
			{ name: 'Import from Strapi and Sanity', detail: 'From their exports, with a dry run; and export.', status: planned('D') },
			{ name: 'Extension points', detail: 'Custom field types declared as data and Studio panels in sandboxed iframes, instead of server plugins.', status: planned('D') }
		]
	},
	{
		id: 'automation',
		title: 'Automation and operations',
		lead: 'Scripts and agents get limited, budgeted access; operators get backups that are proven by a restore.',
		features: [
			{ name: 'Scoped, budgeted tokens and an MCP adapter', detail: 'Agents can draft and propose; they can never approve their own work.', status: planned('P5') },
			{ name: 'Bulk jobs that resume', detail: 'An interrupted bulk edit resumes without duplicates or overwrites.', status: planned('P5') },
			{ name: 'Optional AI writing assistance', detail: 'Off by default, built last.', status: planned('P5') },
			{
				name: 'Several servers behind a load balancer',
				detail: 'Kills and drains under load, rate limits shared across instances, and a stress run for every load scenario.',
				status: planned('S')
			},
			{
				name: 'Install without building Rust',
				detail: 'One version for the server, CLI, SDK and MCP server; the CLI from npm, Cargo, Homebrew or a script.',
				status: planned('V')
			},
			{ name: 'Backups and a restore drill', detail: 'Encrypted backups and a restore onto clean infrastructure, documented.', status: planned('P6') },
			{ name: 'Signed releases', detail: 'Signed artifacts, an SBOM and a security policy for v0.1.', status: planned('P6') }
		]
	}
];
