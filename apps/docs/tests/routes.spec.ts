// The HTTP reference and the router agree, in both directions. The router is read from the newest
// branch with `git show` (the docs describe it; tests/site.ts NEWEST), and from main to know which
// routes aren't merged yet: those must say Building where they are documented. A route is documented
// as an h3 whose text is `METHOD path`, with `{P}` for the environment prefix. It reads the built
// pages, so it checks exactly what readers see. No browser needed.

import { expect, test } from '@playwright/test';
import { atRef, builtHtml, builtPages, textOf } from './site.ts';

const ROUTER = 'crates/colophon-api/src/lib.rs';
const PREFIX = '/api/v1/projects/{project}/environments/{environment}';
const METHODS = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE'] as const;

/** `GET /livez`, `PUT /api/v1/projects/{project}/roles/{role}`, … from an Axum router's source. */
export function routerRoutes(source: string): Set<string> {
	const routes = new Set<string>();
	for (const chunk of source.split('.route(').slice(1)) {
		const path = /^\s*"([^"]+)"/.exec(chunk)?.[1];
		if (!path) continue;
		// The route's own arguments end where the next chained call or function starts.
		const own = chunk.split(/\.merge\(|\.fallback\(|\bfn\s/)[0] ?? '';
		const methods = [...own.matchAll(/(?:^|[^A-Za-z_])(get|post|put|patch|delete)\(/g)].map((match) => (match[1] ?? '').toUpperCase());
		if (methods.length === 0) throw new Error(`no method for the route ${path} in the router`);
		for (const method of methods) routes.add(`${method} ${path}`);
	}
	return routes;
}

const expand = (route: string) => route.replace(' {P}', ` ${PREFIX}`);

/** Every `METHOD path` documented as an h3, with the page and the part of the page under it. */
function documented() {
	const found: { route: string; page: string; section: string; pageLabel: string }[] = [];
	for (const page of builtPages()) {
		const html = builtHtml(page);
		const pageLabel = /<article[\s\S]*?data-capability="([a-z]+)"/.exec(html)?.[1] ?? '';
		const parts = html.split(/(?=<h[23][\s>])/);
		for (const part of parts) {
			const heading = /^<h3[^>]*>([\s\S]*?)<\/h3>/.exec(part);
			if (!heading) continue;
			const text = textOf(heading[1] ?? '').trim();
			const match = new RegExp(`^(${METHODS.join('|')}) (\\{P\\}\\S*|/\\S+)$`).exec(text);
			if (match) found.push({ route: expand(`${match[1] ?? ''} ${match[2] ?? ''}`), page, section: part, pageLabel });
		}
	}
	return found;
}

const newest = routerRoutes(atRef(ROUTER));
const onMain = routerRoutes(atRef(ROUTER, process.env['COLOPHON_DOCS_MAIN_REF'] ?? 'origin/main'));

test('the router parser reads the router', () => {
	expect(newest.size, 'routes on the newest branch').toBeGreaterThan(40);
	expect(newest.has('GET /livez')).toBe(true);
	expect(newest.has(`POST ${PREFIX}/changesets/{changeset}/publish`)).toBe(true);
	expect(newest.has('DELETE /api/v1/projects/{project}/members/{identity}')).toBe(true);
	expect(onMain.has('GET /api/v1/projects/{project}/roles'), 'roles are not on main yet').toBe(false);
});

test('every documented HTTP route exists in the router', () => {
	const unknown = documented()
		.filter(({ route }) => !newest.has(route))
		.map(({ route, page }) => `${route} (on ${page})`);
	expect(unknown, `documented, but not in ${ROUTER} on the newest branch`).toEqual([]);
});

test('every route in the router is documented exactly once', () => {
	const counts = new Map<string, string[]>();
	for (const { route, page } of documented()) counts.set(route, [...(counts.get(route) ?? []), page]);
	const missing = [...newest].filter((route) => !counts.has(route));
	const twice = [...counts].filter(([, pages]) => pages.length > 1).map(([route, pages]) => `${route}: ${pages.join(', ')}`);
	expect(missing, 'routes the reference doesn’t document').toEqual([]);
	expect(twice, 'routes documented more than once').toEqual([]);
});

test('routes that are only on a branch say Building where they are documented', () => {
	const unmarked = documented()
		.filter(({ route }) => newest.has(route) && !onMain.has(route))
		.filter(({ section, pageLabel }) => pageLabel !== 'building' && !section.includes('data-capability="building"'))
		.map(({ route, page }) => `${route} (on ${page})`);
	expect(unmarked).toEqual([]);
});

test('the all-routes page lists every route', () => {
	const page = textOf(builtHtml('/reference/api/routes/'));
	const missing = [...newest].filter((route) => {
		const [method = '', path = ''] = route.split(' ');
		const short = path.startsWith(PREFIX) ? `{P}${path.slice(PREFIX.length)}` : path;
		return !page.includes(`${method} ${short}`) && !(page.includes(short) && page.includes(method));
	});
	expect(missing).toEqual([]);
});
