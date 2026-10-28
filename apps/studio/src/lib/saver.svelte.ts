// Autosave for one entry: what the editor shows as "Saved", "Saving…" or "Conflict".
//
// - Edits are kept on this device first (drafts.ts), then saved a moment after typing stops,
//   one save at a time; edits made during a save follow it.
// - A save names the revision it builds on (If-Match). If the entry moved on, the server
//   refuses (409) and the saver opens a conflict instead of overwriting anyone.
// - A save that gets no answer is sent again with the same Idempotency-Key, so if the first one
//   did reach the database the server replays its answer instead of saving twice (F02).
// - A save turned away for the rate limit (D-076) waits as long as the server asks, then goes
//   again, key and all; the edits stay on this device meanwhile.

import { ApiError, type Content, type Definition, type Draft, getEntry, type Loaded, newKey, saveEntry } from './api.ts';
import { busyFor, URL_NAME } from './words.ts';
import { canonical, same } from './content.ts';
import { plainError } from './errors.ts';
import { forget, keep, type Kept, type Sent } from './drafts.ts';

export type SaveStatus =
	| 'saved'
	| 'unsaved'
	| 'saving'
	/** The last attempt got no answer; it will be sent again. */
	| 'retrying'
	/** The entry moved on; the person must choose. */
	| 'conflict'
	/** The server refused the content; editing again will retry. */
	| 'refused'
	/** The session ended; edits stay on this device until sign-in. */
	| 'signed-out'
	/** Something typed can't be stored as it is (JSON that doesn't read yet): not sent, and not
	 *  kept on this device, until it's fixed. */
	| 'held';

/** A save that was refused because someone else's revision is now the draft. */
export interface Conflict {
	/** What the server holds now. */
	theirs: Draft;
	/** Its ETag: "keep mine" saves on top of it. */
	etag: string;
	/** What this editor has. */
	mine: Content;
	/** True when "mine" came from this device after a reload, rather than a refused save. */
	restored: boolean;
}

/** Quiet time after the last keystroke before saving. */
export const DEBOUNCE_MS = 1000;
/** Waits between attempts that got no answer. */
const RETRY_MS = [1_000, 2_000, 5_000, 10_000, 30_000];

export class Saver {
	status = $state<SaveStatus>('saved');
	/** The ETag of the revision the edits build on: the next save's If-Match. */
	etag = $state('');
	message = $state('');
	conflict = $state.raw<Conflict | null>(null);
	/** False when this browser refused to keep the edits locally. */
	keptOnDevice = $state(true);
	/** Waiting out a rate limit (D-076): `message` says for how long. */
	busy = $state(false);
	/** For a refused save: the field the server named (its JSON Pointer's first part), if any,
	 *  and for a list, the place in it (`/related/1`). */
	field = $state<string | null>(null);
	place = $state<number | null>(null);
	/** The whole JSON Pointer of a refused save (`/sections/1/by`), for controls inside structures,
	 *  and the refusal's reason (`pattern_mismatch`), for words that fit the control. */
	pointer = $state<string | null>(null);
	reason = $state<string | null>(null);
	/** The entry that has a value this one may not share (`value_taken`, `slug_taken`), when the
	 *  server names it: only to a role that reads its title (D-070). */
	holder = $state<{ entry: string; title: string } | null>(null);

	#saved: Content;
	#latest: Content;
	#timer: ReturnType<typeof setTimeout> | undefined;
	#inFlight = false;
	#again = false;
	#key = '';
	#keyFor = '';
	#retries = 0;
	/** No save goes before this time (ms since the epoch): the server asked to wait until then. */
	#notBefore = 0;
	/** Saving is held until the next edit (see `hold`). */
	#held = false;
	/** The request sent and not yet answered: kept on the device so a reload can resend it. */
	#sent: Sent | null = null;
	/** After reopening: the newest edits, applied once the resent request is answered. */
	#newest: Content | null = null;

	constructor(
		readonly project: string,
		readonly collection: string,
		readonly entry: string,
		private readonly definition: Definition,
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
	restore(kept: Kept, server: Draft): Content {
		const { content, base, sent } = kept;
		if (sent !== undefined) {
			this.etag = sent.base;
			this.#latest = sent.content;
			this.#newest = content;
			this.#key = sent.key;
			this.#keyFor = `${sent.base}\n${canonical(this.definition, sent.content)}`;
			// The base's content is unknown here; nothing compares equal to it until the answer.
			this.#saved = { '\u0000': true };
			this.message = 'Restored unsaved changes from this device.';
			this.status = 'unsaved';
			this.saveNow();
			return content;
		}
		if (this.#same(content, this.#saved)) {
			forget(this.project, this.entry);
			return this.#saved;
		}
		if (base === this.etag) {
			this.message = 'Restored unsaved changes from this device.';
			this.edit(content);
			return content;
		}
		this.#latest = content;
		this.conflict = { theirs: server, etag: this.etag, mine: content, restored: true };
		this.status = 'conflict';
		this.message = 'Your unsaved changes on this device were made on an older version.';
		return this.#saved;
	}

	#keep(base: string, content: Content): void {
		this.keptOnDevice = keep(this.project, this.entry, {
			base,
			content,
			at: new Date().toISOString(),
			...(this.#sent === null ? {} : { sent: this.#sent })
		});
	}

	/** Holds saving while something typed can't be stored: says why, sends nothing, and keeps
	 *  nothing on this device (leaving the page warns). The next `edit` carries on. */
	hold(reason: string): void {
		if (this.conflict !== null || this.status === 'signed-out') return;
		clearTimeout(this.#timer);
		this.#held = true;
		this.status = 'held';
		this.message = reason;
		this.keptOnDevice = false;
	}

	/** Records the editor's current content. */
	edit(content: Content): void {
		this.field = null;
		this.place = null;
		this.pointer = null;
		this.reason = null;
		this.holder = null;
		if (this.#held) {
			this.#held = false;
			this.message = '';
		}
		this.#latest = content;
		this.#newest = null;
		if (this.conflict !== null) {
			this.conflict = { ...this.conflict, mine: content };
			this.#keep(this.conflict.etag, content);
			return;
		}
		if (!this.#inFlight && this.#sent === null && this.#same(content, this.#saved)) {
			clearTimeout(this.#timer);
			forget(this.project, this.entry);
			this.status = 'saved';
			return;
		}
		this.#keep(this.etag, content);
		if (this.status === 'signed-out') return;
		if (!this.#inFlight && this.status !== 'retrying') this.status = 'unsaved';
		this.#schedule(this.status === 'retrying' ? Math.max(RETRY_MS[0] ?? 1_000, this.#notBefore - Date.now()) : DEBOUNCE_MS);
	}

	/** Saves at once (the Save button, ⌘S), or as soon as the server said it may. */
	saveNow(): void {
		clearTimeout(this.#timer);
		const wait = this.#notBefore - Date.now();
		if (wait > 0) this.#schedule(wait);
		else void this.#save();
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
	useTheirs(): Content {
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

	#same(a: Content, b: Content): boolean {
		return same(this.definition, a, b);
	}

	destroy(): void {
		clearTimeout(this.#timer);
	}

	#schedule(ms: number): void {
		clearTimeout(this.#timer);
		this.#timer = setTimeout(() => void this.#save(), ms);
	}

	async #save(): Promise<void> {
		if (this.conflict !== null || this.status === 'signed-out' || this.#held) return;
		if (this.#inFlight) {
			this.#again = true;
			return;
		}
		const content = this.#latest;
		if (this.#same(content, this.#saved)) {
			this.status = 'saved';
			forget(this.project, this.entry);
			return;
		}
		// The same request (same base, same content) keeps its key, so a retry can be replayed.
		const fingerprint = `${this.etag}\n${canonical(this.definition, content)}`;
		if (fingerprint !== this.#keyFor) {
			this.#key = newKey();
			this.#keyFor = fingerprint;
		}
		this.#inFlight = true;
		if (this.status !== 'retrying') this.status = 'saving';
		// Kept before it leaves, so a reload can send this exact request again.
		this.#sent = { base: this.etag, key: this.#key, content };
		this.#keep(this.etag, this.#newest ?? this.#latest);
		try {
			const result = await saveEntry(this.project, this.collection, this.entry, content, this.etag, this.#key);
			this.#sent = null;
			this.etag = result.etag;
			this.#saved = result.draft.content;
			this.#retries = 0;
			this.#notBefore = 0;
			this.busy = false;
			this.#keyFor = '';
			this.message = this.#newest === null ? '' : this.message;
			this.onSaved(result);
			if (this.#newest !== null) {
				this.#latest = this.#newest;
				this.#newest = null;
			}
			if (this.#same(this.#latest, this.#saved)) {
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
			// Held while this save was out: say so, whatever it answered.
			if (this.#held) this.status = 'held';
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
					const theirs = await getEntry(this.project, this.collection, this.entry);
					const sent = this.#sent;
					this.#sent = null;
					const mine = this.#newest ?? this.#latest;
					this.#newest = null;
					this.#latest = mine;
					// The current version is what this editor sent: an earlier attempt committed.
					if (this.#same(theirs.draft.content, mine) || (sent !== null && this.#same(theirs.draft.content, sent.content))) {
						this.etag = theirs.etag;
						this.#saved = theirs.draft.content;
						this.onSaved(theirs);
						if (this.#same(mine, this.#saved)) {
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
				if (error.reason === 'rate_limited') this.#wait(error.retryAfter ?? 1_000);
				else this.#retry(error.message);
				return;
			case 'signed-out':
				this.status = 'signed-out';
				this.message = 'Your session ended. Your changes are kept on this device; sign in again to save them.';
				return;
			case 'refused':
			case 'not-found':
				this.#sent = null;
				this.status = 'refused';
				// The API names it a slug; the Studio says URL name (D-059), and which entry has it.
				// Other refusals in plain words; the field they name is marked in the editor.
				const pointer = /^\/([^/]+)(?:\/(\d+))?/.exec(error.details?.pointer ?? '');
				this.field = pointer?.[1] ?? null;
				this.place = pointer?.[2] === undefined ? null : Number(pointer[2]);
				this.pointer = error.details?.pointer ?? null;
				this.reason = error.reason;
				const named = error.details?.entries?.[0];
				this.holder = named === undefined ? null : { entry: named.entry, title: named.title };
				this.message =
					error.reason === 'slug_taken'
						? `another entry already uses this ${URL_NAME}${error.details?.entries?.[0] ? ` (“${error.details.entries[0].title}”)` : ''}: choose another`
						: plainError(error, 'change this entry');
				return;
		}
	}

	/** Turned away for the rate limit: the same request again once the server's wait is over. */
	#wait(ms: number): void {
		this.status = 'retrying';
		this.busy = true;
		this.message = busyFor(ms / 1000, true);
		this.#notBefore = Date.now() + ms;
		this.#schedule(ms);
	}

	#retry(message: string): void {
		this.status = 'retrying';
		this.busy = false;
		this.message = message;
		this.#schedule(RETRY_MS[Math.min(this.#retries, RETRY_MS.length - 1)] ?? 30_000);
		this.#retries += 1;
	}
}
