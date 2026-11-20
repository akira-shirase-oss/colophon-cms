import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';
import { unseeded } from '@colophon/site-kit';

// A fully prerendered SvelteKit site (D-220): every page is built once, at one release, and the rebuild
// webhook builds the next release. `strict` fails the build on any page it can't prerender.
export default defineConfig({
	plugins: [
		sveltekit({
			compilerOptions: {
				runes: ({ filename }) => (filename.split(/[/\\]/).includes('node_modules') ? undefined : true)
			},
			// dist/ like the Astro sites; scripts/serve-demo-sites.sh builds into dist.next and swaps it in.
			adapter: adapter({ pages: process.env['SITE_OUT_DIR'] ?? 'dist', assets: process.env['SITE_OUT_DIR'] ?? 'dist', strict: true }),
			prerender: {
				handleHttpError: 'fail',
				handleMissingId: 'fail',
				// Before the demo project is seeded there are no recipes, so the recipe route has no pages; the
				// build still succeeds and draws the "seed first" page, as the Astro demos do. Once seeded, a
				// prerenderable route that produced no page is an error.
				handleUnseenRoutes: ({ message }) => {
					if (!unseeded()) throw new Error(message);
				}
			}
		})
	]
});
