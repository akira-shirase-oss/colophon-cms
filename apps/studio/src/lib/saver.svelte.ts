// Autosave for one article: what the editor shows as "Saved", "Saving…" or "Conflict".
//
// - Edits are kept on this device first (drafts.ts), then saved a moment after typing stops,
//   one save at a time; edits made during a save follow it.
// - A save names the revision it builds on (If-Match). If the article moved on, the server
//   refuses (409) and the saver opens a conflict instead of overwriting anyone.
// - A save that gets no answer is sent again with the same Idempotency-Key, so if the first one
//   did reach the database the server replays its answer instead of saving twice (F02).

import { serialize } from '@colophon/editor';
import { ApiError, type Article, type Draft, getArticle, type Loaded, newKey, saveArticle } from './api.ts';
import { forget, keep } from './drafts.ts';

export type SaveStatus =
	| 'saved'
	| 'unsaved'
	| 'saving'
	/** The last attempt got no answer; it will be sent again. */
	| 'retrying'
	/** The article moved on; the person must choose. */
	| 'conflict'
	/** The server refused the content; editing again will retry. */
	| 'refused'
	/** The session ended; edits stay on this device until sign-in. */
	| 'signed-out';

/** A save that was refused because someone else's revision is now the draft. */
export interface Conflict {
	/** What the server holds now. */
	theirs: Draft;
	/** Its ETag: "keep mine" saves on top of it. */
	etag: string;
	/** What this editor has. */
	mine: Article;
	/** True when "mine" came from this device after a reload, rather than a refused save. */
	restored: boolean;
}

/** Quiet time after the last keystroke before saving. */
export const DEBOUNCE_MS = 1000;
/** Waits between attempts that got no answer. */
const RETRY_MS = [1_000, 2_000, 5_000, 10_000, 30_000];

function same(a: Article, b: Article): boolean {
	return a.title === b.title && serialize(a.body) === serialize(b.body);
}

export class Saver {
	status = $state<SaveStatus>('saved');
	/** The ETag of the revision the edits build on: the next save's If-Match. */
	etag = $state('');
	message = $state('');
	conflict = $state.raw<Conflict | null>(null);
	/** False when this browser refused to keep the edits locally. */
	keptOnDevice = $state(true);

	#saved: Article;
	#latest: Article;
	#timer: ReturnType<typeof setTimeout> | undefined;
	#inFlight = false;
	#again = false;
	#key = '';
	#keyFor = '';
	#retries = 0;

	constructor(
		readonly project: string,
		readonly entry: string,
		loaded: Loaded,
		private readonly onSaved: (loaded: Loaded) => void = () => {}
	) {
		this.etag = loaded.etag;
		this.#saved = loaded.draft.content;
		this.#latest = loaded.draft.content;
	}

	/** Edits kept on this device from an earlier visit, on top of revision `base`. Returns what
	 *  the editor should show: the kept edits if they build on the current revision, otherwise
	 *  the server's (and the conflict screen opens). */
	restore(article: Article, base: string, server: Draft): Article {
		if (same(article, this.#saved)) {
			forget(this.project, this.entry);
			return this.#saved;
		}
		if (base === this.etag) {
			this.message = 'Restored unsaved changes from this device.';
			this.edit(article);
			return article;
		}
		this.#latest = article;
		this.conflict = { theirs: server, etag: this.etag, mine: article, restored: true };
		this.status = 'conflict';
		this.message = 'Your unsaved changes on this device were made on an older version.';
		return this.#saved;
	}

	/** Records the editor's current content. */
	edit(article: Article): void {
		this.#latest = article;
		if (this.conflict !== null) {
			this.conflict = { ...this.conflict, mine: article };
			this.keptOnDevice = keep(this.project, this.entry, { base: this.conflict.etag, article, at: new Date().toISOString() });
			return;
		}
		if (!this.#inFlight && same(article, this.#saved)) {
			clearTimeout(this.#timer);
			forget(this.project, this.entry);
			this.status = 'saved';
			return;
		}
		this.keptOnDevice = keep(this.project, this.entry, { base: this.etag, article, at: new Date().toISOString() });
		if (this.status === 'signed-out') return;
		if (!this.#inFlight && this.status !== 'retrying') this.status = 'unsaved';
		this.#schedule(this.status === 'retrying' ? RETRY_MS[0] ?? 1_000 : DEBOUNCE_MS);
	}

	/** Saves at once (the Save button, ⌘S). */
	saveNow(): void {
		clearTimeout(this.#timer);
		void this.#save();
	}

	/** Conflict choice: save my version on top of theirs. Theirs stays in the history. */
	keepMine(): void {
		const conflict = this.conflict;
		if (conflict === null) return;
		this.etag = conflict.etag;
		this.#saved = conflict.theirs.content;
		this.conflict = null;
		this.message = '';
		this.status = 'unsaved';
		this.keptOnDevice = keep(this.project, this.entry, { base: this.etag, article: this.#latest, at: new Date().toISOString() });
		this.saveNow();
	}

	/** Conflict choice: drop my version and continue from theirs, which this returns. */
	useTheirs(): Article {
		const conflict = this.conflict;
		if (conflict === null) return this.#latest;
		this.etag = conflict.etag;
		this.#saved = conflict.theirs.content;
		this.#latest = conflict.theirs.content;
		this.conflict = null;
		this.message = '';
		this.status = 'saved';
		forget(this.project, this.entry);
		return this.#saved;
	}

	destroy(): void {
		clearTimeout(this.#timer);
	}

	#schedule(ms: number): void {
		clearTimeout(this.#timer);
		this.#timer = setTimeout(() => void this.#save(), ms);
	}

	async #save(): Promise<void> {
		if (this.conflict !== null || this.status === 'signed-out') return;
		if (this.#inFlight) {
			this.#again = true;
			return;
		}
		const article = this.#latest;
		if (same(article, this.#saved)) {
			this.status = 'saved';
			forget(this.project, this.entry);
			return;
		}
		// The same request (same base, same content) keeps its key, so a retry can be replayed.
		const fingerprint = `${this.etag}\n${article.title}\n${serialize(article.body)}`;
		if (fingerprint !== this.#keyFor) {
			this.#key = newKey();
			this.#keyFor = fingerprint;
		}
		this.#inFlight = true;
		if (this.status !== 'retrying') this.status = 'saving';
		try {
			const result = await saveArticle(this.project, this.entry, article, this.etag, this.#key);
			this.etag = result.etag;
			this.#saved = result.draft.content;
			this.#retries = 0;
			this.#keyFor = '';
			this.message = '';
			this.onSaved(result);
			if (same(this.#latest, this.#saved)) {
				this.status = 'saved';
				forget(this.project, this.entry);
			} else {
				this.status = 'unsaved';
				this.keptOnDevice = keep(this.project, this.entry, { base: this.etag, article: this.#latest, at: new Date().toISOString() });
				this.#schedule(DEBOUNCE_MS);
			}
		} catch (error) {
			if (!(error instanceof ApiError)) throw error;
			await this.#failed(error);
		} finally {
			this.#inFlight = false;
			if (this.#again) {
				this.#again = false;
				if (this.status === 'unsaved' || this.status === 'saving') this.#schedule(0);
			}
		}
	}

	async #failed(error: ApiError): Promise<void> {
		switch (error.failure) {
			case 'conflict':
				try {
					const theirs = await getArticle(this.project, this.entry);
					this.conflict = { theirs: theirs.draft, etag: theirs.etag, mine: this.#latest, restored: false };
					this.status = 'conflict';
					this.message = 'Someone saved a newer version while you were editing.';
				} catch {
					this.#retry('Someone saved a newer version; loading it failed, trying again.');
				}
				return;
			case 'retry':
				this.#retry(error.message);
				return;
			case 'signed-out':
				this.status = 'signed-out';
				this.message = 'Your session ended. Your changes are kept on this device; sign in again to save them.';
				return;
			case 'refused':
			case 'not-found':
				this.status = 'refused';
				this.message = error.message;
				return;
		}
	}

	#retry(message: string): void {
		this.status = 'retrying';
		this.message = message;
		this.#schedule(RETRY_MS[Math.min(this.#retries, RETRY_MS.length - 1)] ?? 30_000);
		this.#retries += 1;
	}
}
