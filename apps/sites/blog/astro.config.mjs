import { defineConfig } from 'astro/config';
import node from '@astrojs/node';

// One of the five category demo sites (apps/sites/README.md): its pages prerender like any
// static site (`export const prerender = true` on each), but /signin, /api/* and /members/*
// render per request because they read the visitor's own audience session — so the whole site
// runs under the node adapter and `node dist/server/entry.mjs` serves what `astro build` wrote.
export default defineConfig({
	output: 'server',
	adapter: node({ mode: 'standalone' })
});
