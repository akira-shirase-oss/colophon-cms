// Installing the website starter (W.0): the section-style component, the pages/navigation/theme
// content types and the three starter entries, each through the ordinary API so every rule and
// permission applies. The plan names what a run would add and what it would leave alone, so the
// dialog can show it before anything happens; running again after a partial install just fills
// in what's missing.

import { HOME_PAGE, navigationSeed, SITE_COMPONENTS, SITE_TYPES, THEME_SEED } from '@colophon/site-template';
import {
	createComponent,
	createContentType,
	createEntry,
	listComponents,
	listContentTypes,
	listEntries,
	type ComponentDefinition,
	type Definition
} from './api.ts';

export interface SitePlan {
	/** Human-readable lines for what a run would create. */
	creates: string[];
	/** And what it would leave alone, already there. */
	keeps: string[];
	/** Nothing left to do. */
	done: boolean;
}

const key = () => crypto.randomUUID().replaceAll('-', '');

/** What a run of `installSite` would do right now. */
export async function planSite(project: string): Promise<SitePlan> {
	const [types, components] = await Promise.all([listContentTypes(project), listComponents(project)]);
	const haveType = new Set(types.map((type) => type.slug));
	const haveComponent = new Set(components.map((shared) => shared.slug));
	const creates: string[] = [];
	const keeps: string[] = [];
	for (const shared of SITE_COMPONENTS) (haveComponent.has(shared.slug) ? keeps : creates).push(`the ${shared.definition.name} component`);
	for (const type of SITE_TYPES) (haveType.has(type.slug) ? keeps : creates).push(`the ${type.definition.name} type`);
	if (haveType.has('pages') && (await listEntries(project, 'pages')).some((entry) => entry.revision.title === HOME_PAGE.title)) keeps.push('the Home page');
	else creates.push('the Home page');
	if (haveType.has('navigation') && (await listEntries(project, 'navigation')).length > 0) keeps.push('the Navigation entry');
	else creates.push('the Navigation entry');
	if (haveType.has('theme') && (await listEntries(project, 'theme')).length > 0) keeps.push('the Theme entry');
	else creates.push('the Theme entry');
	return { creates, keeps, done: creates.length === 0 };
}

/** Creates what's missing of the website starter; reports the Home entry for the "open it" link. */
export async function installSite(project: string, onStep: (line: string) => void = () => {}): Promise<{ home: string }> {
	const [types, components] = await Promise.all([listContentTypes(project), listComponents(project)]);
	const haveType = new Set(types.map((type) => type.slug));
	const haveComponent = new Set(components.map((shared) => shared.slug));
	for (const shared of SITE_COMPONENTS) {
		if (haveComponent.has(shared.slug)) continue;
		onStep(`Adding ${shared.definition.name}`);
		await createComponent(project, shared.slug, shared.definition as ComponentDefinition, key());
	}
	for (const type of SITE_TYPES) {
		if (haveType.has(type.slug)) continue;
		onStep(`Adding ${type.definition.name}`);
		await createContentType(project, type.slug, type.definition as Definition, key());
	}
	const entries = await listEntries(project, 'pages');
	let home = entries.find((entry) => entry.revision.title === HOME_PAGE.title)?.id;
	if (home === undefined) {
		onStep('Writing the Home page');
		home = (await createEntry(project, 'pages', HOME_PAGE, key())).draft.id;
	}
	if ((await listEntries(project, 'navigation')).length === 0) {
		onStep('Writing Navigation');
		await createEntry(project, 'navigation', navigationSeed(home), key());
	}
	if ((await listEntries(project, 'theme')).length === 0) {
		onStep('Writing the Theme');
		await createEntry(project, 'theme', THEME_SEED, key());
	}
	return { home };
}
