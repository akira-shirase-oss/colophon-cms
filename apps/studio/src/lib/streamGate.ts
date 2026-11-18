// Reads wait for the project's live stream (F9). The stream (`live.ts`) never replays, so a commit that lands
// between a page's read and the stream starting to listen would go unheard. Until now the first open therefore
// re-read everything the page had loaded: every resource twice on every full page load. Instead, a project's
// reads wait (briefly) until its stream is open: the server subscribes before it answers the stream's request,
// so any read sent after the `open` event either sees a commit or hears its hint, and nothing needs re-reading.
//
// The wait is bounded: if the stream isn't open within OPEN_WAIT_MS (a slow network, a stream refused at its
// cap), reads go ahead and are marked early, and the stream's first open re-reads as before. Once a project's
// stream has opened, its reads never wait again; a reconnect after the stream's lifetime resyncs in `live.ts`.

/** How long a project read waits for the stream to open before going ahead (and being re-read at the open). */
export const OPEN_WAIT_MS = 400;

interface Gate {
	opened: boolean;
	/** A read went ahead before the stream opened, so the first open must re-read. */
	early: boolean;
	ready: Promise<void>;
	release: () => void;
}

const gates = new Map<string, Gate>();

function gateFor(project: string): Gate {
	let gate = gates.get(project);
	if (gate === undefined) {
		let release = () => {};
		const ready = new Promise<void>((resolve) => (release = resolve));
		gate = { opened: false, early: false, ready, release };
		gates.set(project, gate);
	}
	return gate;
}

/** Resolves when a read of `project` may be sent: at once once its stream is open, else when it opens or after
 *  `OPEN_WAIT_MS`, whichever comes first (then the read is early, and the first open re-reads). */
export async function readsMayStart(project: string, waitMs = OPEN_WAIT_MS): Promise<void> {
	const gate = gateFor(project);
	if (gate.opened) return;
	let timer: ReturnType<typeof setTimeout> | undefined;
	await Promise.race([gate.ready, new Promise<void>((resolve) => (timer = setTimeout(resolve, waitMs)))]);
	clearTimeout(timer);
	if (!gate.opened) gate.early = true;
}

/** The stream for `project` is open: waiting reads go. Returns whether any read went ahead before it (those
 *  may have missed a commit, so the caller re-reads); later calls for the same gate report nothing early. */
export function streamOpened(project: string): { early: boolean } {
	const gate = gateFor(project);
	const early = !gate.opened && gate.early;
	gate.opened = true;
	gate.release();
	return { early };
}

/** The stream for `project` is gone (the person left the project): the next visit waits for a new one. */
export function streamClosed(project: string): void {
	gates.get(project)?.release();
	gates.delete(project);
}

/** The project a read belongs to, from its path, or null for reads outside a project (or the stream itself). */
export function projectOf(path: string): string | null {
	const match = /^\/api\/v1\/projects\/([^/?#]+)\//.exec(path);
	if (match === null || /\/events(?:$|\?)/.test(path)) return null;
	return decodeURIComponent(match[1]!);
}
