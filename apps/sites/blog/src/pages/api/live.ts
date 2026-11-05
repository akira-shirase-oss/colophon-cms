// `GET /api/live`: this browser's live-generation stream, fanned out from the one upstream
// connection to Colophon (D-139). Same-origin, so no CORS and no credentials cross an origin
// boundary; the payload is a generation number and nothing else.

import type { APIRoute } from 'astro';
import { site } from '@colophon/site-kit';
import { currentGeneration, start, subscribe } from '../../lib/live.ts';

export const prerender = false;

export const GET: APIRoute = ({ request }) => {
	const cms = site();
	if (cms === null) return new Response('not seeded\n', { status: 503 });
	start(cms, 'posts');

	const encoder = new TextEncoder();
	let closed = false;
	const stream = new ReadableStream<Uint8Array>({
		start(controller) {
			const send = (generation: number) => {
				if (closed) return;
				controller.enqueue(encoder.encode(`event: generation\ndata: {"generation":${generation}}\n\n`));
			};
			send(currentGeneration());
			const unsubscribe = subscribe(send);
			const heartbeat = setInterval(() => {
				if (!closed) controller.enqueue(encoder.encode(': keep-alive\n\n'));
			}, 15_000);
			request.signal.addEventListener('abort', () => {
				closed = true;
				clearInterval(heartbeat);
				unsubscribe();
				try {
					controller.close();
				} catch {
					// Already closed by the runtime.
				}
			});
		}
	});

	return new Response(stream, {
		headers: {
			'content-type': 'text/event-stream',
			'cache-control': 'no-store',
			'x-accel-buffering': 'no'
		}
	});
};
