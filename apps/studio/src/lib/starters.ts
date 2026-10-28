// Starting points for a new project (step 3.7, D-059): a few plain choices instead of a schema
// designer on day one. Each is applied through the same content-type API anyone could call, as
// the project's new owner, right after the project is made; nothing here is a server feature.

import {
	ApiError,
	type ContentType,
	createContentType,
	type Definition,
	type FieldDefinition,
	getContentType,
	newKey,
	updateContentType
} from './api.ts';

export type Starter = 'blog' | 'docs' | 'blank';

export interface StarterInfo {
	id: Starter;
	name: string;
	/** One line about what it is for. */
	description: string;
	/** What it adds, in words. */
	includes: string;
}

export const STARTERS: StarterInfo[] = [
	{
		id: 'blog',
		name: 'A small blog',
		description: 'Articles for a news page or a personal site.',
		includes: 'Articles with a title, a URL name and a body.'
	},
	{
		id: 'docs',
		name: 'A documentation site',
		description: 'Pages for a help centre or a product guide.',
		includes: 'Pages with a title, a URL name and a body, beside Articles.'
	},
	{
		id: 'blank',
		name: 'Start blank',
		description: 'Set up your own kinds of content later, in Content types.',
		includes: 'Articles with a title and a body.'
	}
];

const title: FieldDefinition = { id: 'title', label: 'Title', type: 'text', required: true, max_chars: 200, multiline: false };
/** A required URL name, suggested from the field `source`. */
const urlNameFrom = (source: string): FieldDefinition => ({ id: 'slug', label: 'URL name', type: 'slug', required: true, source });
const urlName = urlNameFrom('title');
const body: FieldDefinition = { id: 'body', label: 'Body', type: 'rich_text', required: true };

/** Pages for a documentation site. */
export const PAGES: Definition = { name: 'Pages', title_field: 'title', fields: [title, urlName, body] };

/** Every project starts with Articles (a title and a body); the blog gives it a URL name, placed
 *  after the title, unless it has one already. */
export function withUrlName(articles: Definition): Definition {
	if (articles.fields.some((field) => field.type === 'slug')) return articles;
	const at = articles.fields.findIndex((field) => field.id === articles.title_field) + 1;
	const fields = [...articles.fields];
	fields.splice(at, 0, urlNameFrom(articles.title_field));
	return { ...articles, fields };
}

/** Applies `starter` to a project made a moment ago. Safe to run again: each step checks what is
 *  there first, and keys make a lost answer safe to resend. */
export async function applyStarter(project: string, starter: Starter): Promise<void> {
	if (starter === 'blank') return;
	if (starter === 'blog') {
		const articles: ContentType = await getContentType(project, 'articles');
		const next = withUrlName(articles.definition);
		if (next !== articles.definition) await updateContentType(project, 'articles', next, `"v${articles.version}"`, newKey());
		return;
	}
	const pages = await getContentType(project, 'pages').catch((error: unknown) => {
		if (error instanceof ApiError && error.failure === 'not-found') return null;
		throw error;
	});
	if (pages === null) await createContentType(project, 'pages', PAGES, newKey());
}
