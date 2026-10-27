// The site's fixed facts and links, in one place.

export const NAME = 'Colophon';
export const TAGLINE = 'The open-source CMS that can prove what it published.';
export const DESCRIPTION =
	'Colophon is an open-source content management system in Rust and Svelte. Every publish is one numbered release with a receipt that says where it was seen, and nothing is lost when a process is killed. Pre-release: v0.1 is not out yet.';

export const GITHUB = 'https://github.com/akira-shirase-oss/colophon-cms';
export const GITHUB_FILE = (path: string) => `${GITHUB}/blob/main/${path}`;
export const GITHUB_COMMIT = (hash: string) => `${GITHUB}/commit/${hash}`;
export const BUILD_LOG = 'https://404human.dev/colophon/';

// The docs are a separate app (apps/docs), served under /docs on the same host. Links to them are
// marked rel="external" so this site's prerenderer doesn't try to build them.
export const DOCS = '/docs/';
export const QUICKSTART = '/docs/quickstart/';

/** The machine every load result so far was measured on (perf/results/*.json). */
export const MACHINE = 'apple-m2-pro-12c';
export const LAPTOP_CAVEAT =
	'Measured on one laptop (Apple M2 Pro, 12 cores, 32 GB) with the load generator, the server and the database on the same machine. These are numbers about the code on that machine, not a claim about production capacity.';

export interface NavLink {
	href: '/product/' | '/compare/' | '/roadmap/' | '/changelog/' | '/blog/' | '/open-source/';
	label: string;
}

export const NAV: NavLink[] = [
	{ href: '/product/', label: 'Product' },
	{ href: '/compare/', label: 'Compare' },
	{ href: '/roadmap/', label: 'Roadmap' },
	{ href: '/changelog/', label: 'Changelog' },
	{ href: '/blog/', label: 'Blog' },
	{ href: '/open-source/', label: 'Open source' }
];
