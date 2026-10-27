// Serves the built docs (build/) the way the website's host will: under /docs/, a folder's
// index.html for its address, a redirect to add the trailing slash, and 404 for anything else.
// `vite preview` can't be used, because it serves SvelteKit's intermediate output and not the
// Pagefind index written into build/ after it. Bound to 127.0.0.1 only.
//
//   node scripts/serve.js [port] [--with-site]      default 58311
//
// --with-site also serves the website's build (apps/site/build) at the root, as one host would, so
// the links between the two can be followed locally. Without it, / redirects to /docs/.

import { createReadStream, existsSync, statSync } from 'node:fs';
import { createServer } from 'node:http';
import { extname, join, normalize, resolve } from 'node:path';

const docs = resolve(import.meta.dirname, '..', 'build');
const site = resolve(import.meta.dirname, '..', '..', 'site', 'build');
const args = process.argv.slice(2);
const port = Number(args.find((arg) => /^\d+$/.test(arg)) ?? process.env['PORT'] ?? 58311);
const withSite = args.includes('--with-site');
const base = process.env['COLOPHON_DOCS_BASE'] ?? '/docs';

const TYPES = {
	'.html': 'text/html; charset=utf-8',
	'.js': 'text/javascript; charset=utf-8',
	'.mjs': 'text/javascript; charset=utf-8',
	'.css': 'text/css; charset=utf-8',
	'.json': 'application/json',
	'.xml': 'application/atom+xml; charset=utf-8',
	'.svg': 'image/svg+xml',
	'.woff': 'font/woff',
	'.woff2': 'font/woff2',
	'.wasm': 'application/wasm',
	'.txt': 'text/plain; charset=utf-8',
	'.pf_meta': 'application/octet-stream',
	'.pf_index': 'application/octet-stream',
	'.pf_fragment': 'application/octet-stream',
	'.pagefind': 'application/octet-stream'
};

if (!existsSync(join(docs, 'index.html'))) {
	console.error(`error: no build in ${docs}; run npm run build -w @colophon/docs first`);
	process.exit(1);
}
if (withSite && !existsSync(join(site, 'index.html'))) {
	console.error(`error: --with-site, but no build in ${site}; run npm run build -w @colophon/site first`);
	process.exit(1);
}

/** @param {string} path */
function isFile(path) {
	return existsSync(path) && statSync(path).isFile();
}

/**
 * @param {import('node:http').ServerResponse} response
 * @param {string} root
 * @param {string} pathname the path inside root
 * @param {URL} url
 */
function serve(response, root, pathname, url) {
	// Never outside the root.
	const target = normalize(join(root, pathname));
	if (!target.startsWith(root)) {
		response.writeHead(403).end('forbidden');
		return;
	}
	let file = target;
	if (pathname.endsWith('/')) {
		file = join(target, 'index.html');
	} else if (!isFile(target) && isFile(join(target, 'index.html'))) {
		response.writeHead(308, { location: `${url.pathname}/${url.search}` }).end();
		return;
	}
	if (!isFile(file)) {
		response.writeHead(404, { 'content-type': 'text/plain; charset=utf-8' }).end('not found');
		return;
	}
	const type = TYPES[/** @type {keyof typeof TYPES} */ (extname(file))] ?? 'application/octet-stream';
	response.writeHead(200, { 'content-type': type, 'x-content-type-options': 'nosniff' });
	createReadStream(file).pipe(response);
}

createServer((request, response) => {
	const url = new URL(request.url ?? '/', 'http://localhost');
	let pathname;
	try {
		pathname = decodeURIComponent(url.pathname);
	} catch {
		response.writeHead(400).end('bad request');
		return;
	}
	if (pathname === base) {
		response.writeHead(308, { location: `${base}/${url.search}` }).end();
	} else if (pathname.startsWith(`${base}/`)) {
		serve(response, docs, pathname.slice(base.length), url);
	} else if (withSite) {
		serve(response, site, pathname, url);
	} else if (pathname === '/') {
		response.writeHead(307, { location: `${base}/` }).end();
	} else {
		response.writeHead(404, { 'content-type': 'text/plain; charset=utf-8' }).end('not found');
	}
}).listen(port, '127.0.0.1', () => {
	console.log(`serving ${docs} at http://127.0.0.1:${port}${base}/${withSite ? `, and ${site} at /` : ''}`);
});
