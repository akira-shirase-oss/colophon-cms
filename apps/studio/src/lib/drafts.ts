// Unsaved edits, kept on this device until the server has them, so closing the tab, losing the
// network or a server restart never loses an edit (milestone P1). One entry per article.

import type { Article } from './api.ts';

export interface Kept {
	/** The revision the edits were made on top of (its ETag). */
	base: string;
	article: Article;
	/** When it was kept, as an ISO timestamp. */
	at: string;
}

const storageKey = (project: string, entry: string) => `colophon:unsaved:${project}:${entry}`;

/** Kept edits older than this are dropped unread (D-038). */
export const KEEP_DAYS = 30;

/** Keeps `kept`; false if this browser won't store it (private mode, storage full). */
export function keep(project: string, entry: string, kept: Kept): boolean {
	try {
		localStorage.setItem(storageKey(project, entry), JSON.stringify(kept));
		return true;
	} catch {
		return false;
	}
}

/** The edits kept for this article, if any. Anything unreadable counts as nothing. */
export function kept(project: string, entry: string): Kept | null {
	try {
		const text = localStorage.getItem(storageKey(project, entry));
		if (text === null) return null;
		const value = JSON.parse(text) as Partial<Kept>;
		const article = value.article;
		const valid =
			typeof value.base === 'string' &&
			typeof value.at === 'string' &&
			typeof article?.title === 'string' &&
			typeof article.body === 'object' &&
			article.body !== null;
		const age = Date.now() - Date.parse(value.at ?? '');
		if (!valid || !(age < KEEP_DAYS * 86_400_000)) {
			localStorage.removeItem(storageKey(project, entry));
			return null;
		}
		return value as Kept;
	} catch {
		return null;
	}
}

export function forget(project: string, entry: string): void {
	try {
		localStorage.removeItem(storageKey(project, entry));
	} catch {
		// Nothing to forget if storage is unavailable.
	}
}
