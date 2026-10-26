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
import { forget, keep, type Kept, type Sent } from './drafts.ts';

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
	/** The request sent and not yet answered: kept on the device so a reload can resend it. */
	#sent: Sent | null = null;
	/** After reopening: the newest edits, applied once the resent request is answered. */
	#newest: Article | null = null;

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

	/** Edits kept on this device from an earlier visit. Returns what the editor should show.
	 *
	 *  If a save was in flight when the page went away, it is sent again first, exactly, key
	 *  included: if it committed, the server replays it rather than storing it twice. Otherwise
	 *  kept edits on the current revision are restored and saved, and kept edits on an older
	 *  revision open the conflict screen. */
	restore(kept: Kept, server: Draft): Article {
		const { article, base, sent } = kept;
		if (sent !== undefined) {
			this.etag = sent.base;
			this.#latest = sent.article;
			this.#newest = article;
			this.#key = sent.key;
			this.#keyFor = `${sent.base}\n${sent.article.title}\n${serialize(sent.article.body)}`;
			// The base's content is unknown here; nothing compares equal to it until the answer.
			this.#saved = { title: '\u0000', body: sent.article.body };
			this.message = 'Restored unsaved changes from this device.';
			this.status = 'unsaved';
			this.saveNow();
			return article;
		}
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

	#keep(base: string, article: Article): void {
		this.keptOnDevice = keep(this.project, this.entry, {
			base,
			article,
			at: new Date().toISOString(),
			...(this.#sent === null ? {} : { sent: this.#sent })
		});
	}

	/** Records the editor's current content. */
	edit(article: Article): void {
		this.#latest = article;
		this.#newest = null;
		if (this.conflict !== null) {
			this.conflict = { ...this.conflict, mine: article };
			this.#keep(this.conflict.etag, article);
			return;
		}
		if (!this.#inFlight && this.#sent === null && same(article, this.#saved)) {
			clearTimeout(this.#timer);
			forget(this.project, this.entry);
			this.status = 'saved';
			return;
		}
		this.#keep(this.etag, article);
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
		this.#keep(this.etag, this.#latest);
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
		// Kept before it leaves, so a reload can send this exact request again.
		this.#sent = { base: this.etag, key: this.#key, article };
		this.#keep(this.etag, this.#newest ?? this.#latest);
		try {
			const result = await saveArticle(this.project, this.entry, article, this.etag, this.#key);
			this.#sent = null;
			this.etag = result.etag;
			this.#saved = result.draft.content;
			this.#retries = 0;
			this.#keyFor = '';
			this.message = this.#newest === null ? '' : this.message;
			this.onSaved(result);
			if (this.#newest !== null) {
				this.#latest = this.#newest;
				this.#newest = null;
			}
			if (same(this.#latest, this.#saved)) {
				this.status = 'saved';
				forget(this.project, this.entry);
			} else {
				this.status = 'unsaved';
				this.#keep(this.etag, this.#latest);
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
					const sent = this.#sent;
					this.#sent = null;
					const mine = this.#newest ?? this.#latest;
					this.#newest = null;
					this.#latest = mine;
					// The current version is what this editor sent: an earlier attempt committed.
					if (same(theirs.draft.content, mine) || (sent !== null && same(theirs.draft.content, sent.article))) {
						this.etag = theirs.etag;
						this.#saved = theirs.draft.content;
						this.onSaved(theirs);
						if (same(mine, this.#saved)) {
							this.status = 'saved';
							forget(this.project, this.entry);
						} else {
							this.status = 'unsaved';
							this.#keep(this.etag, mine);
							this.#schedule(DEBOUNCE_MS);
						}
						return;
					}
					// The editor already shows `mine`, the newest edits, even after a reopen.
					this.conflict = { theirs: theirs.draft, etag: theirs.etag, mine, restored: false };
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
				this.#sent = null;
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
