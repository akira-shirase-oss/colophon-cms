// Serves the built site (build/) the way a plain static host would: a folder's index.html for
// its address, a redirect to add the trailing slash, and 404 for anything else. `vite preview`
// can't be used, because it serves SvelteKit's intermediate output and not the Pagefind index
// written into build/ after it. Bound to 127.0.0.1 only.
//
//   node scripts/serve.js [port]      default 58311

import { createReadStream, existsSync, statSync } from 'node:fs';
import { createServer } from 'node:http';
import { extname, join, normalize, resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..', 'build');
const port = Number(process.argv[2] ?? process.env['PORT'] ?? 58311);

const TYPES = {
	'.html': 'text/html; charset=utf-8',
	'.js': 'text/javascript; charset=utf-8',
	'.mjs': 'text/javascript; charset=utf-8',
	'.css': 'text/css; charset=utf-8',
	'.json': 'application/json',
	'.svg': 'image/svg+xml',
	'.woff2': 'font/woff2',
	'.wasm': 'application/wasm',
	'.txt': 'text/plain; charset=utf-8'
};

if (!existsSync(join(root, 'index.html'))) {
	console.error(`error: no build in ${root}; run npm run build -w @colophon/docs first`);
	process.exit(1);
}

/** @param {string} path */
function isFile(path) {
	return existsSync(path) && statSync(path).isFile();
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
	// Never outside build/.
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
}).listen(port, '127.0.0.1', () => {
	console.log(`serving ${root} on http://127.0.0.1:${port}/`);
});
