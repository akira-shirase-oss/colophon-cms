import { changelog } from '#lib/server/changelog.ts';
import type { PageServerLoad } from './$types';

// CHANGELOG.md, parsed at build time (lib/server/changelog.ts). A malformed entry fails the build.
export const load: PageServerLoad = () => ({ changelog: changelog() });
