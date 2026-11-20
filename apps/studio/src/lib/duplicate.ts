// Duplicating an entry: everything the role can read, with "(copy)" after the title and, when the
// type has a URL name, the next free one ("-2", "-3"…; the server's uniqueness rule, D-054).
// Fields the role can't read aren't copied. The editor's More menu uses it, as the collection's
// rows do.

import { ApiError, type Content, createEntry, type Definition, type Loaded, newKey } from './api.ts';

/** Makes the copy of `content` (an entry of a type with `definition`) and answers it. */
export async function duplicateEntry(project: string, collection: string, definition: Definition, source: Content): Promise<Loaded> {
	const content: Content = { ...source };
	const titleField = definition.fields.find((field) => field.id === definition.title_field);
	const title = content[definition.title_field];
	if (typeof title === 'string') {
		const limit = titleField?.type === 'text' ? titleField.max_chars : 200;
		content[definition.title_field] = `${title.slice(0, Math.max(0, limit - 7))} (copy)`;
	}
	const slug = definition.fields.find((field) => field.type === 'slug' && typeof content[field.id] === 'string' && content[field.id] !== '');
	const stem = slug === undefined ? '' : String(content[slug.id]).replace(/-\d+$/, '');
	for (let attempt = 2; ; attempt += 1) {
		if (slug !== undefined) content[slug.id] = `${stem}-${attempt}`;
		try {
			return await createEntry(project, collection, content, newKey());
		} catch (failure) {
			if (!(slug !== undefined && failure instanceof ApiError && failure.reason === 'slug_taken' && attempt < 50)) throw failure;
		}
	}
}
