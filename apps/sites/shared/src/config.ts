// Which Colophon project a demo site reads (apps/sites/README.md): the environment variables
// COLOPHON_API, COLOPHON_PROJECT and COLOPHON_ENVIRONMENT when they are set, else the
// `apps/sites/.colophon.json` file `scripts/seed-demo-sites.sh` writes. A site built before
// seeding gets `null` and draws the "run the seed" placeholder instead of failing, so
// `npm run build` works on a clean checkout and in CI where no server runs.

import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

/** Where a site reads: the API origin, the project's id and the environment's slug. */
export interface Site {
	api: string;
	project: string;
	environment: string;
}

interface Seeded {
	api?: unknown;
	project?: unknown;
	environment?: unknown;
}

/** The seed's record, `apps/sites/.colophon.json`, relative to the site's own directory
 *  (`process.cwd()` while `astro build` runs). */
const RECORD = '../.colophon.json';

function fromRecord(): Site | null {
	let parsed: Seeded;
	try {
		parsed = JSON.parse(readFileSync(resolve(process.cwd(), RECORD), 'utf8')) as Seeded;
	} catch {
		return null;
	}
	if (typeof parsed.api !== 'string' || typeof parsed.project !== 'string') return null;
	return {
		api: parsed.api,
		project: parsed.project,
		environment: typeof parsed.environment === 'string' ? parsed.environment : 'production'
	};
}

/** The site's connection, or `null` when nothing has been seeded yet. */
export function site(): Site | null {
	const env = process.env;
	if (env['COLOPHON_API'] !== undefined && env['COLOPHON_PROJECT'] !== undefined) {
		return {
			api: env['COLOPHON_API'],
			project: env['COLOPHON_PROJECT'],
			environment: env['COLOPHON_ENVIRONMENT'] ?? 'production'
		};
	}
	return fromRecord();
}

/** True when the build should draw the placeholder: the usual case on a clean checkout. */
export function unseeded(): boolean {
	return site() === null;
}
