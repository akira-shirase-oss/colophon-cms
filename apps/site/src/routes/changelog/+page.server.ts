import { changelog } from '#lib/server/changelog.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = () => ({ changelog });
