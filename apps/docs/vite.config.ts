import { execFileSync } from 'node:child_process';
import adapter from '@sveltejs/adapter-static';
import tailwindcss from '@tailwindcss/vite';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';
import { markdown } from './src/lib/markdown/mdsvex.ts';

/** The commit the site is built from, printed in every page's footer: labels describe this code. */
function commit(): string {
	try {
		return execFileSync('git', ['rev-parse', '--short', 'HEAD'], { encoding: 'utf8' }).trim();
	} catch {
		return 'unknown';
	}
}

export default defineConfig({
	define: { __COMMIT__: JSON.stringify(commit()) },
	plugins: [
		// The design system (D-056): Tailwind CSS v4 before SvelteKit, as its docs require.
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
			adapter: adapter({ strict: true })
		})
	],
	server: {
		host: '127.0.0.1',
		port: 58310,
		strictPort: true,
		// The design documents and load results are read from the repository at build time.
		fs: { allow: ['../..'] }
	}
	// No `preview` here: `npm run preview` serves build/ itself (scripts/serve.js) on 58311, because
	// Vite's preview doesn't serve the Pagefind index written after the build.
});
