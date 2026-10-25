import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

export default defineConfig({
	plugins: [
		sveltekit({
			compilerOptions: {
				runes: ({ filename }) =>
					filename.split(/[/\\]/).includes('node_modules') ? undefined : true
			},
			// A static single-page app; colophon-server will serve it on its own origin (AGENTS.md).
			adapter: adapter({ fallback: '200.html' })
		})
	],
	server: { host: '127.0.0.1', port: 58200, strictPort: true },
	preview: { host: '127.0.0.1', port: 58200, strictPort: true }
});
