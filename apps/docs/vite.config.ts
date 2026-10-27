import { execFileSync } from 'node:child_process';
import adapter from '@sveltejs/adapter-static';
import tailwindcss from '@tailwindcss/vite';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';
import { markdown } from './src/lib/markdown/mdsvex.ts';
import { DOCS_BASE, isSitePath } from './src/lib/markdown/addresses.ts';

/** The commit the site is built from, printed in every page's footer: labels describe this code. */
function commit(): string {
	try {
		return execFileSync('git', ['rev-parse', '--short', 'HEAD'], { encoding: 'utf8' }).trim();
	} catch {
		return 'unknown';
	}
}

export default defineConfig({
	define: {
		__COMMIT__: JSON.stringify(commit()),
		__BASE__: JSON.stringify(DOCS_BASE)
	},
	plugins: [
		// The design system (D-056) and the website's look: Tailwind CSS v4 before SvelteKit.
		tailwindcss(),
		sveltekit({
			extensions: ['.svelte', '.md'],
			preprocess: [markdown()],
			compilerOptions: {
				// mdsvex writes legacy-mode components (`$$props`, a module script), so Markdown pages and
				// dependencies choose their own mode; everything written here is in runes mode.
				runes: ({ filename }) =>
					filename.endsWith('.md') || filename.split(/[/\\]/).includes('node_modules') ? undefined : true
			},
			// Every page is prerendered to plain HTML, served from one origin with no server code
			// (module 16). Pagefind indexes the output after the build.
			adapter: adapter({ strict: true }),
			// The docs live under /docs on the website's host (docs/modules/20-site.md), so every
			// address and asset starts with the base path. Absolute, not relative, paths: the search
			// index and the feed name pages by their full address.
			paths: { base: DOCS_BASE, relative: false },
			prerender: {
				// A link to a missing page or a missing #anchor fails the build. Links out to the website
				// are checked against its routes instead of being crawled (it is a different app).
				handleHttpError: ({ path, referrer, message }) => {
					if (!path.startsWith(`${DOCS_BASE}/`) && isSitePath(path)) return;
					throw new Error(`${message}${referrer ? ` (linked from ${referrer})` : ''}`);
				},
				handleMissingId: 'fail'
			}
		})
	],
	server: {
		host: '127.0.0.1',
		port: 58310,
		strictPort: true,
		// The design documents, the changelog and load results are read from the repository at build time.
		fs: { allow: ['../..'] }
	}
	// No `preview` here: `npm run preview` serves build/ itself (scripts/serve.js) on 58311, because
	// Vite's preview doesn't serve the Pagefind index written after the build.
});
