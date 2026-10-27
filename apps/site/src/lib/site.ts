// The site's fixed facts and links, in one place.

export const NAME = 'Colophon';
export const TAGLINE = 'The open-source CMS that can prove what it published.';
export const DESCRIPTION =
	'Colophon is an open-source content management system in Rust and Svelte. Every publish is one numbered release with a receipt that says where it was seen, and nothing is lost when a process is killed. Pre-release: v0.1 is not out yet.';

export const GITHUB = 'https://github.com/akira-shirase-oss/colophon-cms';
export const GITHUB_FILE = (path: string) => `${GITHUB}/blob/main/${path}`;
export const GITHUB_COMMIT = (hash: string) => `${GITHUB}/commit/${hash}`;
export const BUILD_LOG = 'https://404human.dev/colophon/';
/** The project's account on X, where steps are announced. */
export const X_ACCOUNT = 'https://x.com/akirashiraseoss';

// The docs are a separate app (apps/docs), built for /docs on the same host. Links to them are
// marked rel="external" so this site's prerenderer doesn't try to build them; the docs' tests check
// that every address here is one of their pages.
export const DOCS = '/docs/';
export const QUICKSTART = '/docs/get-started/quickstart/';
/** The docs' rendering of CHANGELOG.md and its Atom feed. */
export const DOCS_CHANGELOG = '/docs/changelog/';
export const CHANGELOG_FEED = '/docs/changelog/feed.xml';

/** The machine every load result so far was measured on (perf/results/*.json). */
export const MACHINE = 'apple-m2-pro-12c';
export const LAPTOP_CAVEAT =
	'Measured on one laptop (Apple M2 Pro, 12 cores, 32 GB) with the load generator, the server and the database on the same machine. These are numbers about the code on that machine, not a claim about production capacity.';

export interface NavLink {
	href: string;
	label: string;
	/** A link into the docs app, which this site's prerenderer must not follow. */
	external?: boolean;
}

/** The bar's pages, in the order a first visit reads them. */
export const NAV: NavLink[] = [
	{ href: '/product/', label: 'Product' },
	{ href: '/compare/', label: 'Compare' },
	{ href: '/roadmap/', label: 'Roadmap' },
	{ href: DOCS, label: 'Docs', external: true },
	{ href: '/blog/', label: 'Blog' },
	{ href: '/changelog/', label: 'Changelog' }
];
