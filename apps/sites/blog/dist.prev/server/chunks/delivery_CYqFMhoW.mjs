// ../shared/src/delivery.ts
var PAGE = 100;
/** A bound on paging: more pages than this means something is wrong, so the build stops. */
var MAX_PAGES = 50;
/**
* Every published entry of `collection` the caller may read: anonymous without `token`, or as
* the site user a live audience session names — whose role the database applies (D-137). A
* member read is answered `private` straight from Postgres, never the anonymous cache. Throws
* on a refused or failed answer: a seeded site that cannot read its API should fail the build,
* not go live silently empty.
*/
async function entries(site, collection, token) {
	const base = `${site.api}/content/v1/${site.project}/${site.environment}/${collection}`;
	const headers = { accept: "application/json" };
	if (token !== void 0) headers.authorization = `Bearer ${token}`;
	const items = [];
	let generation = 0;
	let cursor = null;
	for (let page = 0; page < MAX_PAGES; page += 1) {
		const url = new URL(base);
		url.searchParams.set("limit", String(PAGE));
		if (cursor !== null) url.searchParams.set("cursor", cursor);
		const response = await fetch(url, { headers });
		if (!response.ok) throw new Error(`GET ${url.pathname}: HTTP ${response.status}`);
		const named = Number(response.headers.get("colophon-generation"));
		if (Number.isFinite(named)) generation = named;
		const body = await response.json();
		items.push(...body.data ?? []);
		cursor = body.next_cursor ?? null;
		if (cursor === null) break;
	}
	return {
		items,
		generation
	};
}
/**
* One published entry by id — the same read `entries` makes, narrowed. `null` when the caller
* may not read it (gated reads are indistinguishable from unpublished ones) or it is gone.
*/
async function entry(site, collection, id, token) {
	const url = `${site.api}/content/v1/${site.project}/${site.environment}/${collection}/${id}`;
	const headers = { accept: "application/json" };
	if (token !== void 0) headers.authorization = `Bearer ${token}`;
	const response = await fetch(url, { headers });
	if (response.status === 404 || response.status === 410) return null;
	if (!response.ok) throw new Error(`GET ${new URL(url).pathname}: HTTP ${response.status}`);
	return (await response.json()).data ?? null;
}
/** The public URL of a media asset: `/media/v1/{project}/{environment}/{asset}[/{rendition}]`. */
function mediaUrl(site, asset, rendition) {
	const base = `${site.api}/media/v1/${site.project}/${site.environment}/${asset}`;
	return rendition === void 0 ? base : `${base}/${rendition}`;
}
export { entry as n, mediaUrl as r, entries as t };
