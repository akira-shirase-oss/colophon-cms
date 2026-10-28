import adapter from '@sveltejs/adapter-static';
import tailwindcss from '@tailwindcss/vite';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

// Where the built Studio goes: `build`, which the server serves; STUDIO_BUILD_DIR builds elsewhere,
// so tests can run against new work while a server keeps serving the last build.
const out = process.env['STUDIO_BUILD_DIR'] ?? 'build';

export default defineConfig({
	plugins: [
		// The design system (D-053): Tailwind CSS v4 before SvelteKit, as its docs require.
		tailwindcss(),
		sveltekit({
			compilerOptions: {
				runes: ({ filename }) =>
					filename.split(/[/\\]/).includes('node_modules') ? undefined : true
			},
			// A static single-page app that colophon-server serves at /studio on its own origin, so the
			// session cookie and the CSRF token need no CORS (D-037). Absolute asset paths, because
			// the fallback page answers for every route.
			adapter: adapter({ pages: out, assets: out, fallback: '200.html' }),
			paths: { base: '/studio', relative: false }
		})
	],
	server: {
		host: '127.0.0.1',
		port: 58200,
		strictPort: true,
		// In development, the API is a colophon-server on its default port.
		proxy: Object.fromEntries(
			['/api', '/auth', '/me', '/livez', '/readyz'].map((path) => [path, 'http://127.0.0.1:58000'])
		)
	},
	preview: { host: '127.0.0.1', port: 58200, strictPort: true }
});
