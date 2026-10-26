// Seeds the local demo (scripts/demo.sh): signs in as the development realm's dev-owner the way a
// browser does, then creates a project with a few articles about Colophon itself, points a site
// webhook and a probe at the site runner, publishes three articles as release 1 and leaves two
// more in a changeset waiting for review, so every flow can be tried from the Studio. Run with
// Node 24: node scripts/demo-seed.ts <server origin> <site origin>. Prints the project's id.

const [origin = 'http://localhost:58000', site = 'http://127.0.0.1:58099'] = process.argv.slice(2);

const cookies = new Map<string, string>();

function remember(response: Response): void {
	for (const line of response.headers.getSetCookie()) {
		const [pair = ''] = line.split(';');
		const at = pair.indexOf('=');
		if (at > 0) cookies.set(pair.slice(0, at).trim(), pair.slice(at + 1).trim());
	}
}

function cookieHeader(): string {
	return [...cookies].map(([name, value]) => `${name}=${value}`).join('; ');
}

/** Follows redirects by hand, keeping cookies for every host on the way. */
async function visit(url: string, init: RequestInit = {}): Promise<Response> {
	let next = url;
	let options: RequestInit = init;
	for (let hop = 0; hop < 10; hop += 1) {
		const response = await fetch(next, { ...options, redirect: 'manual', headers: { ...(options.headers ?? {}), cookie: cookieHeader() } });
		remember(response);
		const location = response.headers.get('location');
		if (response.status < 300 || response.status >= 400 || location === null) return response;
		next = new URL(location, next).toString();
		options = {};
	}
	throw new Error('too many redirects');
}

async function signIn(): Promise<void> {
	const form = await visit(`${origin}/auth/login?return_to=/me`);
	const action = /<form[^>]*id="kc-form-login"[^>]*action="([^"]+)"/.exec(await form.text())?.[1];
	if (action === undefined) throw new Error('no Keycloak sign-in form');
	const signedIn = await visit(action.replaceAll('&amp;', '&'), {
		method: 'POST',
		headers: { 'content-type': 'application/x-www-form-urlencoded' },
		body: new URLSearchParams({ username: 'dev-owner', password: 'colophon-dev' })
	});
	if (signedIn.status !== 200) throw new Error(`sign-in failed: HTTP ${signedIn.status}`);
}

async function api<T>(method: string, path: string, body?: unknown, headers: Record<string, string> = {}): Promise<{ status: number; data: T }> {
	const response = await fetch(`${origin}${path}`, {
		method,
		headers: {
			cookie: cookieHeader(),
			origin,
			'x-csrf-token': cookies.get('colophon_csrf') ?? '',
			...(body === undefined ? {} : { 'content-type': 'application/json' }),
			...headers
		},
		...(body === undefined ? {} : { body: JSON.stringify(body) })
	});
	const text = await response.text();
	const parsed = text === '' ? {} : (JSON.parse(text) as { data?: T });
	if (response.status >= 400) throw new Error(`${method} ${path}: HTTP ${response.status} ${text}`);
	return { status: response.status, data: parsed.data as T };
}

const key = () => crypto.randomUUID().replaceAll('-', '');

function richText(...paragraphs: string[]): unknown {
	return {
		schema: 'colophon/rich-text',
		version: 1,
		doc: { type: 'doc', content: paragraphs.map((text) => ({ type: 'paragraph', content: [{ type: 'text', text }] })) }
	};
}

const ARTICLES: Array<[string, string[]]> = [
	['What Colophon is for', ['A content management system that can prove what it published.', 'Every publish becomes a numbered release.']],
	['Releases you can name', ['Release 7 always means the same content.', 'Old releases stay readable, and nothing changes them.']],
	['Taking something down', ['Unpublishing is reviewed like any change.', 'Once released, old links to it stop working too.']],
	['Receipts, not spinners', ['A probe looks at the live site after every release.', 'Verified always says how many places looked.']],
	['Killing it on purpose', ['The worker is killed in the middle of its work, over and over.', 'Nothing is lost and nothing happens twice.']]
];

async function main(): Promise<void> {
	await signIn();
	const project = (await api<{ id: string }>('POST', '/api/v1/projects', { slug: `demo-${Date.now().toString(36)}`, name: 'Demo site' })).data.id;
	const base = `/api/v1/projects/${project}/environments/production`;
	await api('POST', `${base}/destinations`, { name: 'Site build', url: `${site}/hook` });
	await api('POST', `${base}/destinations`, { kind: 'probe', name: 'Live site', url: `${site}/` });
	const ids: string[] = [];
	for (const [title, paragraphs] of ARTICLES) {
		const created = await api<{ id: string }>('POST', `${base}/collections/articles/entries`, { title, body: richText(...paragraphs) }, { 'idempotency-key': key() });
		ids.push(created.data.id);
	}
	const changeset = async (title: string, entries: string[]) =>
		(await api<{ id: string; hash: string }>('POST', `${base}/changesets`, { title, entries }, { 'idempotency-key': key() })).data;
	const first = await changeset('Launch', ids.slice(0, 3));
	await api('POST', `${base}/changesets/${first.id}/submit`, undefined, { 'if-match': `"${first.hash}"` });
	await api('POST', `${base}/changesets/${first.id}/reviews`, { decision: 'approve', comment: 'Ready.' }, { 'if-match': `"${first.hash}"` });
	await api('POST', `${base}/changesets/${first.id}/publish`, { base_generation: 0 }, { 'if-match': `"${first.hash}"`, 'idempotency-key': key() });
	const second = await changeset('Second wave', ids.slice(3));
	await api('POST', `${base}/changesets/${second.id}/submit`, undefined, { 'if-match': `"${second.hash}"` });
	console.log(project);
}

await main();
