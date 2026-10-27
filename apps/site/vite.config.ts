import adapter from '@sveltejs/adapter-static';
import tailwindcss from '@tailwindcss/vite';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

// The site's public address, used only where an absolute URL is required (sitemap.xml and the
// blog's RSS feed). Hosting is undecided (O-012), so the default is a reserved name that can't
// resolve; set COLOPHON_SITE_ORIGIN when building for a real host.
const origin = process.env['COLOPHON_SITE_ORIGIN'] ?? 'https://colophon.invalid';

export default defineConfig({
	plugins: [
		// The design system (D-056): Tailwind CSS v4 before SvelteKit, as its docs require.
		tailwindcss(),
		sveltekit({
			compilerOptions: {
				runes: ({ filename }) =>
					filename.split(/[/\\]/).includes('node_modules') ? undefined : true
			},
			// Every page is prerendered to plain HTML (+layout.ts). The fallback page is what a static
			// host serves for an unknown address; it renders the site's 404 in the browser.
			adapter: adapter({ fallback: '404.html' }),
			paths: { origin },
			prerender: {
				// A broken internal link or a missing #anchor fails the build.
				handleHttpError: 'fail',
				handleMissingId: 'fail'
			}
		})
	],
	server: { host: '127.0.0.1', port: 58300, strictPort: true },
	preview: { host: '127.0.0.1', port: 58301, strictPort: true }
});
