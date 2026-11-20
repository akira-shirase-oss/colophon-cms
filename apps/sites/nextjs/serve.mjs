// Serves the prerendered site in dist/ on 127.0.0.1 (scripts/serve-demo-sites.sh; PORT, default 58116).
// Static files only: `/x/` is dist/x/index.html, a missing file is 404, nothing outside dist/ is served.
import { createServer } from 'node:http';
import { existsSync, readFileSync, statSync } from 'node:fs';
import { extname, join, normalize } from 'node:path';

const root = join(import.meta.dirname, 'dist');
const port = Number(process.env['PORT'] ?? 58116);
const TYPES = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript', '.css': 'text/css', '.json': 'application/json', '.svg': 'image/svg+xml', '.png': 'image/png', '.webp': 'image/webp', '.woff2': 'font/woff2', '.txt': 'text/plain' };

createServer((request, response) => {
	const path = decodeURIComponent(new URL(request.url ?? '/', 'http://x').pathname);
	let file = normalize(join(root, path));
	if (!file.startsWith(root)) return void response.writeHead(400).end();
	if (existsSync(file) && statSync(file).isDirectory()) file = join(file, 'index.html');
	if (!existsSync(file)) return void response.writeHead(404, { 'content-type': 'text/plain' }).end('Not found');
	response.writeHead(200, { 'content-type': TYPES[extname(file)] ?? 'application/octet-stream' }).end(readFileSync(file));
}).listen(port, '127.0.0.1', () => console.log(`nextjs demo on http://127.0.0.1:${port}/`));
