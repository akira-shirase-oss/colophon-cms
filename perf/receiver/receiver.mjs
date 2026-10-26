// A webhook and CDN destination for the publishing load test (scripts/load.sh publishing): answers
// every POST (announcements to /hook, purges to /purge) with 204 and counts what it got, by
// idempotency key. GET /stats returns the counts, so
// the scenario can check that every announcement arrived and how many were repeats.
// Usage: node perf/receiver/receiver.mjs <port>

import { createServer } from 'node:http';

const port = Number(process.argv[2] ?? '58096');
let requests = 0;
const keys = new Set();

createServer((request, response) => {
	if (request.method === 'GET' && request.url === '/stats') {
		response.writeHead(200, { 'content-type': 'application/json' });
		response.end(JSON.stringify({ requests, distinct_keys: keys.size }));
		return;
	}
	request.resume();
	request.on('end', () => {
		requests += 1;
		keys.add(String(request.headers['idempotency-key'] ?? ''));
		response.writeHead(204).end();
	});
}).listen(port, '127.0.0.1', () => console.log(`receiver listening on 127.0.0.1:${port}`));
