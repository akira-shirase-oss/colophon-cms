// Before `next build`: learn the newest release once and write it to .release.json, which every build worker
// reads (lib/colophon.ts). The connection comes from COLOPHON_API, COLOPHON_PROJECT and COLOPHON_ENVIRONMENT, or
// from apps/sites/.colophon.json (scripts/seed-demo-sites.sh). Not seeded: no file, and the site builds one page
// that says to seed first.
import { existsSync, readFileSync, rmSync, writeFileSync } from 'node:fs';

const out = '.release.json';
rmSync(out, { force: true });

function connection() {
	const env = process.env;
	if (env.COLOPHON_API && env.COLOPHON_PROJECT) {
		return { api: env.COLOPHON_API, project: env.COLOPHON_PROJECT, environment: env.COLOPHON_ENVIRONMENT ?? 'production' };
	}
	if (!existsSync('../.colophon.json')) return null;
	const seeded = JSON.parse(readFileSync('../.colophon.json', 'utf8'));
	return { api: seeded.api, project: seeded.project, environment: seeded.environment ?? 'production' };
}

const site = connection();
if (site === null) {
	console.log('pin-release: nothing seeded; building the "seed first" page');
} else {
	const url = `${site.api}/content/v1/${site.project}/${site.environment}/recipes?limit=1`;
	const response = await fetch(url, { headers: { accept: 'application/json' }, signal: AbortSignal.timeout(10_000) });
	if (!response.ok) throw new Error(`GET ${new URL(url).pathname}: HTTP ${response.status}`);
	const generation = Number(response.headers.get('colophon-generation'));
	if (!Number.isInteger(generation) || generation < 1) throw new Error('the delivery API named no release');
	writeFileSync(out, JSON.stringify({ site, generation }) + '\n');
	console.log(`pin-release: this build reads release ${generation}`);
}
