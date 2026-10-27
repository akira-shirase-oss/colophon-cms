// The site's pages, for the tests and the screenshots.
export const PAGES = [
	{ path: '/', name: 'home' },
	{ path: '/product/', name: 'product' },
	{ path: '/compare/', name: 'compare' },
	{ path: '/roadmap/', name: 'roadmap' },
	{ path: '/changelog/', name: 'changelog' },
	{ path: '/blog/', name: 'blog' },
	{ path: '/blog/checkpoint-3-4/', name: 'blog-post' },
	{ path: '/open-source/', name: 'open-source' },
	{ path: '/no-such-page/', name: '404' }
] as const;
