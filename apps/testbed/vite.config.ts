import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig, type ProxyOptions } from 'vite';

// The testbed reaches colophon-server through this proxy: `/server/livez` becomes
// `<COLOPHON_SERVER>/livez`. The browser sees a single origin, so the server needs no CORS
// (AGENTS.md: CORS stays narrowly scoped). Headers pass through untouched.
const target = process.env['COLOPHON_SERVER'] ?? 'http://127.0.0.1:58000';
const proxy: Record<string, ProxyOptions> = {
	'/server': {
		target,
		rewrite: (path) => path.replace(/^\/server/, '')
	}
};

export default defineConfig({
	plugins: [
		sveltekit({
			compilerOptions: {
				// Runes mode for the project's own components.
				runes: ({ filename }) =>
					filename.split(/[/\\]/).includes('node_modules') ? undefined : true
			},
			// A static single-page app: the testbed has no server code of its own.
			adapter: adapter({ fallback: '200.html' })
		})
	],
	server: { host: '127.0.0.1', port: 58100, strictPort: true, proxy },
	preview: { host: '127.0.0.1', port: 58100, strictPort: true, proxy }
});
