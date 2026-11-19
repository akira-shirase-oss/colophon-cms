<!--
  The entry editor (steps 1.3, 1.5 and 3.5): every field of the entry's content type as a row,
  rich text with its toolbar, autosave with a status anyone can read, and, when someone else
  saved first, a choice instead of an overwrite. History compares any version with this draft.
  The title is the page's heading, edited in place (D-184). Beside the fields, three blocks
  (D-184, after D-081): a status card (where it stands for readers, in one line, and the save
  state), the one next step with ⋯ More, and the entry's language and its Details, closed until
  asked. Comments open in a drawer beside the content, so the text stays in view.
-->
<script lang="ts">
	import { onMount, tick } from 'svelte';
	import type { Editor } from '@tiptap/core';
	import { createQueries, createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { resolve } from '$app/paths';
	import ArrowUpRightIcon from '@lucide/svelte/icons/arrow-up-right';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import CopyIcon from '@lucide/svelte/icons/copy';
	import EyeIcon from '@lucide/svelte/icons/eye';
	import LinkIcon from '@lucide/svelte/icons/link';
	import MessageSquareIcon from '@lucide/svelte/icons/message-square';
	import WifiOffIcon from '@lucide/svelte/icons/wifi-off';
	import XIcon from '@lucide/svelte/icons/x';
	import { toast } from 'svelte-sonner';
	import { openEditor, type RichTextValue, toStored } from '@colophon/editor';
	import RichText from '@colophon/content-renderer/RichText.svelte';
	import CloudAlertIcon from '@lucide/svelte/icons/cloud-alert';
	import CloudCheckIcon from '@lucide/svelte/icons/cloud-check';
	import LoaderIcon from '@lucide/svelte/icons/loader-circle';
	import { Button } from '@colophon/ui/ui/button';
	import { goto } from '$app/navigation';
	import {
		ApiError,
		assetUrl as mediaUrl,
		type Content,
		type ContentType,
		deliveryUrl,
		type Draft,
		type FieldDefinition,
		getChangeset,
		getRevision,
		listRevisions,
		listSchedules,
		type Loaded,
		may,
		mayOn,
		newKey,
		readerAbsence,
		type Referrer,
		type Revision,
		signInUrl,
		type Summary,
		type Undone,
		undoRelease
	} from './api.ts';
	import { linkedQuery, placeName } from './references.ts';
	import { clearedNote, shownTitle } from './words.ts';
	import { readable } from './diff.ts';
	import { prettyJson } from './json.ts';
	import { plainError } from './errors.ts';
	import CommentsPanel from './CommentsPanel.svelte';
	import EntryHistory from './EntryHistory.svelte';
	import EntryLanguage from './EntryLanguage.svelte';
	import EntryTitle from './EntryTitle.svelte';
	import PublishEntry from './PublishEntry.svelte';
	import ReaderView from './ReaderView.svelte';
	import SiteCanvas from './SiteCanvas.svelte';
	import ThemeLook from './ThemeLook.svelte';
	import { changesetsQuery, meQuery, projectQuery, releasesQuery, typesQuery } from './queries.ts';
	import RichTextToolbar from './RichTextToolbar.svelte';
	import { imageFileHandlers, imageView } from './richTextImage.svelte.ts';
	import StatusBadge from './shell/StatusBadge.svelte';
	import TechnicalDetails from './shell/TechnicalDetails.svelte';
	import { releaseName } from './words.ts';
	import { applies, blockingProblem, EMPTY_DOCUMENT, type FormValue, formValue, storedValue, titleLabel } from './content.ts';
	import { kept } from './drafts.ts';
	import { duplicateEntry } from './duplicate.ts';
	import FieldInputs from './FieldInputs.svelte';
	import { clock } from './history.ts';
	import { type InReview, publishInOneClick, newKeys } from './publishing.ts';
	import { Saver } from './saver.svelte.ts';
	import { STATUS, type Status, type Tone } from './status.ts';
	import PreviewMenu from './PreviewMenu.svelte';
	import PreviewPane from './PreviewPane.svelte';
	import PreviewSplit from './PreviewSplit.svelte';

	let {
		project,
		collection,
		entry,
		type,
		initial,
		published = null,
		referencedBy = []
	}: {
		project: string;
		collection: string;
		entry: string;
		type: ContentType;
		initial: Loaded;
		/** What readers get now, from the collection's list; null when not published. */
		published?: Summary['published'];
		/** Entries that link to this one, as its read says (D-065). */
		referencedBy?: Referrer[];
	} = $props();

	// "Linked from": each linking entry named by reading it; one the role can't read is said so.
	const allTypes = createQuery(() => typesQuery(project));
	const linking = createQueries(() => ({ queries: referencedBy.map((link) => linkedQuery(project, link.collection, link.entry_id)) }));
	const linkedFrom = $derived(
		referencedBy.map((link, index) => {
			const definition = allTypes.data?.find((item) => item.slug === link.collection)?.definition;
			return {
				link,
				read: linking[index]?.data,
				collectionName: definition?.name ?? link.collection,
				// "Author", or inside structures "Sections → Quote 2 → By" (the kind from its content).
				fieldLabel: placeName(definition?.fields ?? [], link.field, linking[index]?.data?.content) || link.field
			};
		})
	);
	/** Published entries that link here: taking this down is refused until they change. */
	const linkedLive = $derived(
		linkedFrom.filter((item) => item.link.published).map((item) => (item.read?.seen ? `“${shownTitle(item.read.title)}”` : 'an entry you can’t see'))
	);

	const queryClient = useQueryClient();
	// A draft shows each image's current version from the media origin (gap 3 D1); a published
	// release's own versions are what delivery's `media` names.
	const assetUrl = (assetId: string) => mediaUrl(project, assetId);
	/** A refusal inside a rich-text field, in plain words (D-059); its code is in Technical details. */
	function richRefusal(reason: string | null, message: string): string {
		switch (reason) {
			case 'asset_not_found':
				return "This image isn't in the media library. Choose a replacement or remove it.";
			case 'too_many_images':
				return 'This text has more than 200 different images. Remove some, or split it into more entries.';
			default:
				return message;
		}
	}
	// Images in the editor: the real picture, its state, and Edit alt text / Replace / Remove.
	const images = () => ({ image: { view: imageView(project, queryClient) } });
	// The component is keyed on the entry, so the props it starts with are the ones it keeps.
	// svelte-ignore state_referenced_locally
	const definition = type.definition;
	// What the person's role hides or locks (D-058, step R.3): hidden fields are never shown or
	// sent, so a save can't erase them; locked ones are shown, disabled.
	// svelte-ignore state_referenced_locally
	const hidden = new Set(initial.draft.hidden_fields ?? []);
	// svelte-ignore state_referenced_locally
	const locked = initial.draft.locked_fields ?? [];
	const visible = definition.fields.filter((field) => !hidden.has(field.id));
	/** Fields named by their labels, never their code names (D-147). */
	const labelsOf = (ids: string[]) => ids.map((id) => definition.fields.find((field) => field.id === id)?.label ?? id).join(', ');
	const plain = visible.filter((field) => field.type !== 'rich_text');
	const rich = visible.filter((field) => field.type === 'rich_text');
	/** The title field, when it is one line of text the role reads: it is the page's heading,
	 *  edited in place (D-184), rather than a field among the others. */
	const headingField = visible.find(
		(field): field is Extract<FieldDefinition, { type: 'text' }> => field.id === definition.title_field && field.type === 'text' && !field.multiline
	);
	/** The fields in the definition's order, as the builder lists them (D-147): runs of plain
	 *  fields, each a block of rows, between rich-text fields, each full width where it sits. */
	type Segment = { kind: 'plain'; fields: FieldDefinition[] } | { kind: 'rich'; field: FieldDefinition };
	const segments: Segment[] = [];
	for (const field of visible) {
		if (field === headingField) continue;
		const last = segments.at(-1);
		if (field.type === 'rich_text') segments.push({ kind: 'rich', field });
		else if (last?.kind === 'plain') last.fields.push(field);
		else segments.push({ kind: 'plain', fields: [field] });
	}
	// svelte-ignore state_referenced_locally
	let latest = $state.raw<Draft>(initial.draft);
	// A fresh draft arriving after mount — the entry's query refetched, say a sibling was
	// added elsewhere — replaces what the status row and details show. The form state is
	// untouched: edits live in `form` and the editors.
	$effect(() => {
		latest = initial.draft;
	});
	// Preview on the editor's own site (D-181): the pane, and the last save the server
	// acknowledged, which the pane tells the site about — never a keystroke.
	let previewOpen = $state(false);
	let acknowledged = $state.raw<{ revision: string; at: number } | null>(null);
	// The version this tab's form is editing: the one it opened, then each it saved. When the live read brings
	// a newer one from someone else, the page says so at once (the title is the form, so it keeps the words
	// being edited): the next save then asks which version to keep.
	// svelte-ignore state_referenced_locally
	let editing = $state(initial.draft.revision.id);
	// svelte-ignore state_referenced_locally
	const saver = new Saver(project, collection, entry, definition, initial, (loaded) => {
		latest = loaded.draft;
		editing = loaded.draft.revision.id;
		acknowledged = { revision: loaded.draft.revision.id, at: Date.now() };
		queryClient.setQueryData(['draft', project, collection, entry], loaded);
		void queryClient.invalidateQueries({ queryKey: ['entries', project, collection] });
		void queryClient.invalidateQueries({ queryKey: ['history', project, collection, entry] });
	});
	const savedElsewhere = $derived(latest.revision.id !== editing && saver.conflict === null);
	// What the role may do here, and the newest release this page has seen: publishing goes on top
	// of that one (D-045).
	const mine = createQuery(() => projectQuery(project));
	const who = createQuery(meQuery);
	const releases = createQuery(() => releasesQuery(project));
	const revisions = createQuery(() => ({
		queryKey: ['history', project, collection, entry],
		queryFn: () => listRevisions(project, collection, entry)
	}));

	const hosts: Record<string, HTMLDivElement> = {};
	let editors = $state.raw<Record<string, Editor>>({});
	let form = $state<Record<string, FormValue>>({});
	/** Bumped each time the whole content is (re)shown, so nested controls start from it. */
	let showings = $state(0);
	let unopenable = $state('');

	/** Saves the entry's language (D-087) with the draft, and answers how the save ended. */
	function setLocale(tag: string) {
		saver.locale = tag;
		saver.edit(current());
		return saver.flush();
	}

	// Conditional fields (D-072): a field applies while the field it depends on holds one of its
	// values, judged from what this editor would save. One that doesn't apply is hidden and left
	// out of every save, so a save never earns `not_applicable`.
	const controllers = definition.fields.filter((field) => definition.fields.some((other) => other.when?.field === field.id));
	const deciding = $derived(Object.fromEntries(controllers.map((field) => [field.id, storedValue(field, form[field.id])])));
	const appliesNow = (field: FieldDefinition) => applies(field, deciding);
	/** What was cleared because it stopped applying, said quietly until it applies again. */
	let cleared = $state<Record<string, string>>({});

	function current(): Content {
		const content: Content = {};
		const decided = Object.fromEntries(controllers.map((field) => [field.id, storedValue(field, form[field.id])]));
		for (const field of visible) {
			if (!applies(field, decided)) continue;
			const editor = editors[field.id];
			const value = field.type === 'rich_text' ? editor && toStored(editor.getJSON()) : storedValue(field, form[field.id]);
			if (value !== undefined) content[field.id] = value;
		}
		return content;
	}

	/** Clears what stopped applying, and says so; forgets the note once it applies again. */
	function clearInapplicable() {
		const decided = Object.fromEntries(controllers.map((field) => [field.id, storedValue(field, form[field.id])]));
		for (const field of visible) {
			if (field.when === undefined) continue;
			if (applies(field, decided)) {
				if (cleared[field.id] !== undefined) delete cleared[field.id];
				continue;
			}
			const editor = editors[field.id];
			const filled = field.type === 'rich_text' ? editor !== undefined && !editor.isEmpty : storedValue(field, form[field.id]) !== undefined;
			if (!filled) continue;
			if (field.type === 'rich_text') editor?.commands.clearContent();
			else form[field.id] = formValue(field, undefined);
			const dependsOn = definition.fields.find((other) => other.id === field.when?.field);
			const values = field.when.is.map((value) => (value === true ? 'Yes' : value === false ? 'No' : String(value)));
			cleared[field.id] = clearedNote(field.label, dependsOn?.label ?? field.when.field, values);
		}
	}

	function changed() {
		clearInapplicable();
		// JSON that doesn't read yet can't be sent (it would be lost or stored as text): hold.
		const blocked = blockingProblem(plain.filter(appliesNow), form);
		if (blocked !== '') saver.hold(blocked);
		else saver.edit(current());
	}

	/** Shows `content`, refusing any rich text the editor can't give back exactly (D-022). */
	function show(content: Content) {
		const next: Record<string, Editor> = {};
		for (const field of rich) {
			try {
				next[field.id] = openEditor((content[field.id] as RichTextValue | undefined) ?? EMPTY_DOCUMENT, {
					editorProps: {
						attributes: { role: 'textbox', 'aria-multiline': 'true', 'aria-label': field.label, class: 'prose-colophon min-h-64' },
						...imageFileHandlers
					},
					editable: !locked.includes(field.id),
					onUpdate: changed
				}, images());
			} catch (error) {
				Object.values(next).forEach((editor) => editor.destroy());
				unopenable = `${field.label}: ${(error as Error).message}`;
				return;
			}
		}
		Object.values(editors).forEach((editor) => editor.destroy());
		for (const field of rich) {
			const host = hosts[field.id];
			const editor = next[field.id];
			if (host && editor) editor.mount(host);
		}
		editors = next;
		form = Object.fromEntries(plain.map((field) => [field.id, formValue(field, content[field.id])]));
		// Controls inside structures (and their rich text) are made afresh for the new content.
		showings += 1;
		unopenable = '';
	}

	function keepMine() {
		// After a reload the editor shows the server's version; "mine" is what this device kept.
		if (saver.conflict?.restored) show(saver.conflict.mine);
		saver.keepMine();
	}

	function useTheirs() {
		show(saver.useTheirs());
		editing = latest.revision.id;
		void queryClient.invalidateQueries({ queryKey: ['history', project, collection, entry] });
	}

	// Someone else saved: the history this page loaded is out of date.
	$effect(() => {
		if (saver.conflict !== null) void queryClient.invalidateQueries({ queryKey: ['history', project, collection, entry] });
	});

	function shown(field: FieldDefinition, value: unknown): string {
		if (value === undefined) return '(empty)';
		if (field.type === 'object' || field.type === 'list' || field.type === 'blocks' || field.type === 'reference') return readable(field, value) || '(empty)';
		if (field.type === 'boolean') return value === true ? 'Yes' : 'No';
		if (field.type === 'datetime' && typeof value === 'string') return new Date(value).toLocaleString();
		if (field.type === 'json') return prettyJson(value);
		return String(value);
	}

	// What readers have, by content (F3): a draft whose content is the live version's — restored
	// from History, say — is live, with nothing to publish, though it is a newer version. Versions
	// never change, so the live one is read once.
	const liveVersion = createQuery(() => ({
		queryKey: ['revision', project, collection, entry, published?.revision_id ?? ''],
		queryFn: () => getRevision(project, collection, entry, published?.revision_id ?? ''),
		enabled: published !== null && published.revision_id !== latest.revision.id,
		staleTime: Infinity
	}));
	const matchesLive = $derived(
		published !== null &&
			(published.revision_id === latest.revision.id ||
				(liveVersion.data !== undefined && liveVersion.data.revision.id === published.revision_id && liveVersion.data.revision.content_hash === latest.revision.content_hash))
	);
	const standing = $derived(published === null ? 'draft' : matchesLive && saver.status === 'saved' ? 'live' : 'changed');

	// A draft that a release took down reads "Taken down", not like one never published: the public
	// read of it answers "gone" (410) rather than "not found". Only a release changes that answer, so it
	// is keyed under the project's releases: publishing, unpublishing and the stream's releases hints
	// refresh it, and a save (an entries hint, a list refresh) never reads the public API again.
	const absence = createQuery(() => ({
		queryKey: ['releases', project, 'absence', collection, entry],
		queryFn: () => readerAbsence(project, collection, entry),
		enabled: published === null,
		retry: false
	}));
	const takenDown = $derived(published === null && absence.data === 'gone');

	// A publish request this entry waits in for review. The project's requests are one shared read
	// (the sidebar counts them); the newest few waiting are read in full to find this entry, a
	// bounded number, so an editor never fans out over every request.
	const requests = createQuery(() => changesetsQuery(project));
	// Scheduled ones (D-060) are approved requests with a pending schedule: the project's schedules
	// are one more read, and they count against the same bound.
	const schedules = createQuery(() => ({ queryKey: ['schedules', project], queryFn: () => listSchedules(project) }));
	const pendingRuns = $derived(new Map((schedules.data ?? []).filter((one) => one.state === 'scheduled').map((one) => [one.changeset_id, one.run_at])));
	const waiting = $derived(
		(requests.data ?? [])
			.filter((request) => request.status.state === 'awaiting_review' || (request.status.state === 'approved' && pendingRuns.has(request.id)))
			.sort((a, b) => b.updated_at.localeCompare(a.updated_at))
			.slice(0, 10)
	);
	const waitingReads = createQueries(() => ({
		queries: waiting.map((request) => ({ queryKey: ['changeset', project, request.id], queryFn: () => getChangeset(project, request.id) }))
	}));
	const review = $derived.by((): InReview | null => {
		for (const read of waitingReads) {
			const item = read.data?.items.find((pinned) => pinned.entry_id === entry);
			if (read.data === undefined || item === undefined) continue;
			const runAt = pendingRuns.get(read.data.id) ?? null;
			if (read.data.status.state === 'awaiting_review' || (read.data.status.state === 'approved' && runAt !== null)) {
				return { id: read.data.id, title: read.data.title, unpublish: item.kind === 'unpublish', runAt };
			}
		}
		return null;
	});

	/** The badge: where it stands for readers, or that it waits for review. */
	const shownState = $derived<Status>(
		review !== null ? (review.runAt ? 'scheduled' : 'review') : takenDown && standing === 'draft' ? 'taken_down' : standing
	);
	/** When a scheduled request publishes, in the reader's own clock. */
	const runsAt = $derived(review?.runAt ? new Date(review.runAt).toLocaleString(undefined, { weekday: 'short', day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' }) : '');
	/** The status card's rule, in the badge's tone: the editor's mark for "what readers have". */
	const RULE: Record<Tone, string> = { live: 'bg-live', changed: 'bg-changed', pending: 'bg-pending', attention: 'bg-attention', draft: 'bg-draft' };
	const rule = $derived(RULE[STATUS[shownState].tone]);
	/** The release readers have it from, and when that went out, when this page has it. */
	const liveRelease = $derived.by(() => {
		const live = published;
		return live === null ? undefined : releases.data?.releases.find((release) => release.generation === live.generation);
	});
	/** Where readers get the published version: anyone, without signing in, when it is public. */
	const publicAddress = $derived(`${window.location.origin}${deliveryUrl(project, collection)}/${entry}`);
	/** Who may read the live version (D-137), once this page knows: its gate is pinned with it, so
	 *  it is the live revision's, not the draft's. A gated entry's address is not a public link —
	 *  anyone not signed in as a member gets "not found" — so it is never offered as one. */
	const liveGate = $derived(
		published === null
			? null
			: published.revision_id === latest.revision.id
				? (latest.revision.visibility ?? null)
				: (liveVersion.data?.revision.visibility ?? null)
	);
	const openToAnyone = $derived(liveGate === 'public');
	const gateWords = $derived(
		liveGate === 'authenticated'
			? 'Only readers signed in on your site can read it.'
			: liveGate?.startsWith('role:')
				? `Only readers with the “${liveGate.slice('role:'.length)}” role can read it.`
				: ''
	);
	async function copyAddress(text: string) {
		try {
			await navigator.clipboard.writeText(text);
			toast.success('Copied');
		} catch {
			toast.error("Couldn't copy; select it and copy it yourself.");
		}
	}

	// "What readers get": the public read of this entry, in a dialog.
	let readerView = $state(false);

	// The site canvas (W.2): pages get a Look tab rendering the draft — sections, menu and theme —
	// through the shared renderer, and the theme entry a design sheet with its contrast checked.
	// Clicking a section in a page's canvas opens that section's form.
	// svelte-ignore state_referenced_locally
	const sitePage = collection === 'pages' && definition.fields.some((field) => field.id === 'sections' && field.type === 'blocks');
	// svelte-ignore state_referenced_locally
	const siteTheme = collection === 'theme' && definition.fields.some((field) => field.id === 'light');
	/** The content the canvas renders: exactly what a save would send right now. */
	const canvasContent = $derived(sitePage || siteTheme ? current() : null);

	let tab = $state<'content' | 'visual' | 'history'>('content');
	const tabs = $derived(
		sitePage || siteTheme
			? ([['content', 'Content'], ['visual', 'Look'], ['history', 'History']] as const)
			: ([['content', 'Content'], ['history', 'History']] as const)
	);
	// Writing and answering comments asks for `content.update` on this entry (E1); reading asks
	// for `content.read`, which the route's own read already proved.
	const commentWrite = $derived(mayOn(mine.data, 'content.update', collection, who.data != null && latest.created_by === who.data.id));
	/** Open comments, for a count on the Comments button; the panel reports it as its list lands. */
	let commentCount = $state(0);
	// Comments are a drawer beside the content (D-184), not a tab instead of it: the text stays in
	// view while people talk about it. It doesn't trap focus; Escape closes it and gives focus back.
	let commentsOpen = $state(false);
	let commentsButton = $state<HTMLElement | null>(null);
	let commentsHeading = $state<HTMLElement | null>(null);
	async function openComments(next: boolean) {
		commentsOpen = next;
		await tick();
		if (next) commentsHeading?.focus();
		else commentsButton?.focus();
	}
	async function editSection(index: number) {
		tab = 'content';
		await tick();
		// The sections list's own items (a nested list's, if a section held one, come under it).
		const target = document.querySelector<HTMLElement>(`[data-testid="sections-sections"] > [data-testid="section"]:nth-of-type(${index + 1})`);
		if (target === null) return;
		const toggle = target.querySelector<HTMLButtonElement>('button[aria-expanded]');
		if (toggle?.getAttribute('aria-expanded') === 'false') toggle.click();
		target.scrollIntoView({ block: 'nearest', behavior: smoothScroll() });
		toggle?.focus({ preventScroll: true });
	}

	// Restoring a version (spec's History screen): its fields become the draft and are saved as a new
	// version on top of the current one, so nothing in the history is lost and the restore itself can
	// be undone by restoring again. Fields the role may not change keep their current values; hidden
	// ones are never sent, so the server keeps them.
	const restores = $derived(locked.length < visible.length);
	let restoring = $state<string | null>(null);
	let restored = $state<{ kind: 'done' | 'failed'; text: string; live: boolean } | null>(null);
	/** Puts `old`'s content (fields the role may change) in the editor and saves it. */
	async function putBack(old: Content): Promise<void> {
		const now = current();
		const next: Content = {};
		for (const field of visible) {
			const value = locked.includes(field.id) ? now[field.id] : old[field.id];
			if (value !== undefined) next[field.id] = value;
		}
		show(next);
		saver.edit(current());
		await saver.flush();
	}
	const readVersion = (id: string) =>
		queryClient.fetchQuery({ queryKey: ['revision', project, collection, entry, id], queryFn: () => getRevision(project, collection, entry, id), staleTime: Infinity });
	async function restore(revision: Revision) {
		restoring = revision.id;
		restored = null;
		try {
			const old = await readVersion(revision.id);
			const live = published !== null && (revision.id === published.revision_id || revision.content_hash === liveVersion.data?.revision.content_hash);
			tab = 'content';
			restored = { kind: 'done', text: `Restored the version saved ${clock(revision.created_at)}, as a new version. The newer ones stay in History.`, live };
			await putBack(old.content);
		} catch (failure) {
			restored = { kind: 'failed', text: `Not restored. ${plainError(failure, 'change this entry')}`, live: false };
		} finally {
			restoring = null;
		}
	}
	/** Restoring what readers have leaves nothing to publish, and says so (F3). */
	const restoreNote = $derived(
		restored === null ? null : restored.live && standing === 'live' ? { ...restored, text: 'Back to the live version — nothing to publish.' } : restored
	);

	// Taking it down asks first; afterwards, for ten seconds, Undo puts the version readers had back
	// as a new release: one request (D-184), which the server takes only while that takedown is the
	// newest release, by the person who made it, and only where publishing needs no other review.
	// The draft is never touched — newer changes stay in it, and the entry then reads "changed".
	// One key per toast: a second press, or a resend after a lost answer, replays the one release.
	let undoing = false;
	function unpublished(generation: number) {
		const key = newKey();
		toast.success(`Unpublished in ${releaseName(generation)}`, {
			description: 'Readers no longer get it.',
			duration: 10_000,
			action: { label: 'Undo', onClick: () => void undo(generation, key) }
		});
	}
	/** How many times Undo sends the same request again when no answer came back. */
	const UNDO_RESENDS = 2;
	async function undo(generation: number, key: string) {
		if (undoing) return;
		undoing = true;
		try {
			let done: Undone | null = null;
			for (let attempt = 0; done === null; attempt += 1) {
				try {
					done = await undoRelease(project, generation, key);
				} catch (failure) {
					// No answer: it may have committed, so the same key asks again and gets that answer.
					if (failure instanceof ApiError && failure.failure === 'retry' && failure.status === null && attempt < UNDO_RESENDS) {
						await new Promise((wake) => setTimeout(wake, 400 * (attempt + 1)));
						continue;
					}
					throw failure;
				}
			}
			toast.success(`Back on your site in ${releaseName(done.generation)}`, { description: 'Readers get the version they had before.' });
		} catch (failure) {
			const reason = failure instanceof ApiError ? failure.reason : null;
			const why: Record<string, string> = {
				changed_since: 'Another release went out in the meantime, so this one can’t be undone. Publish it again when it’s ready.',
				undo_expired: 'The moment to undo it has passed. Publish it again instead.',
				not_your_release: 'Only the person who took it down can undo it. Publish it again instead.',
				entry_deleted: 'It has been deleted since, so it can’t come back.'
			};
			if (reason === 'needs_review') {
				toast.error('Not undone', {
					description: 'Publishing here goes through review, so it needs a publish request.',
					action: { label: 'Ask for review', onClick: () => void askToRepublish() }
				});
			} else {
				toast.error('Not undone', { description: (reason !== null && why[reason]) || plainError(failure, 'publish this') });
			}
		} finally {
			undoing = false;
			await Promise.all([
				queryClient.invalidateQueries({ queryKey: ['entries', project] }),
				queryClient.invalidateQueries({ queryKey: ['changesets', project] }),
				queryClient.invalidateQueries({ queryKey: ['releases', project] })
			]);
		}
	}
	/** Where review is required: a publish request for the saved version, sent for review. */
	async function askToRepublish() {
		const done = await publishInOneClick({
			project,
			title: latest.revision.title || 'Untitled',
			entries: [entry],
			oneClick: false,
			seenGeneration: undefined,
			keys: newKeys(),
			step: () => {},
			doing: 'ask for review here'
		});
		if (done.kind === 'requested') toast.success('Sent for review', { description: 'Someone who can publish approves it.' });
		else if (done.kind === 'stopped') toast.error('Not sent', { description: done.message });
		await queryClient.invalidateQueries({ queryKey: ['changesets', project] });
	}

	// Duplicate (⋯ More): a copy of what the role reads, opened in its own editor.
	const duplicates = $derived(definition.single !== true && may(mine.data, 'content.create', collection));
	async function duplicate() {
		try {
			if ((await saver.flush()) !== 'saved') {
				toast.error('Not duplicated', { description: 'Your changes aren’t saved yet; the copy would leave them out.' });
				return;
			}
			const made = await duplicateEntry(project, collection, definition, current());
			toast.success('Duplicated. This is the copy.');
			await queryClient.invalidateQueries({ queryKey: ['entries', project, collection] });
			await goto(resolve('/projects/[project]/[collection]/[entry]', { project, collection, entry: made.draft.id }));
		} catch (failure) {
			toast.error(`Not duplicated. ${plainError(failure, 'create entries here')}`);
		}
	}

	// Details (closed until asked) and, inside it, Technical details: ⋯ More opens both.
	let detailsOpen = $state(false);
	let technicalOpen = $state(false);
	let detailsSummary = $state<HTMLElement | null>(null);
	/** Smooth only for people who haven't asked their system for less motion. */
	const smoothScroll = (): ScrollBehavior => (matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth');
	async function showTechnical() {
		detailsOpen = true;
		technicalOpen = true;
		await tick();
		detailsSummary?.scrollIntoView({ block: 'nearest', behavior: smoothScroll() });
		detailsSummary?.focus({ preventScroll: true });
	}

	/** The save state in a few words, beside the badge; what went wrong is said in full above the
	 *  fields (P3-16), where there is room for it. */
	const saveWord = $derived.by(() => {
		switch (saver.status) {
			case 'saved':
				return 'Saved';
			case 'unsaved':
				return 'Unsaved';
			case 'saving':
				return 'Saving…';
			case 'retrying':
				return saver.busy ? 'Waiting' : 'Not saved yet';
			case 'signed-out':
				return 'Signed out';
			default:
				return 'Not saved';
		}
	});
	/** The reason a save didn't go, said beside the status word: a refusal's or a hold's message, or how long a
	 *  busy server asks to wait. Nothing while saving, saved or merely unsaved. */
	const saveReason = $derived(
		saver.status === 'refused' || saver.status === 'held' || (saver.status === 'retrying' && saver.busy) ? (saver.message ?? '') : ''
	);
	/** Whether the browser says it has no network: "Offline" is then the plain truth. */
	let online = $state(typeof navigator === 'undefined' ? true : navigator.onLine);
	/** What went wrong with saving, in full, for the banner above the fields; null when nothing did. */
	const saveProblem = $derived.by((): { tone: 'pending' | 'attention'; text: string; retry: boolean } | null => {
		const kept = saver.keptOnDevice ? 'your changes are saved on this device and will sync.' : 'your changes aren’t kept on this device, so keep this tab open.';
		switch (saver.status) {
			case 'retrying':
				if (saver.busy) return { tone: 'pending', text: `${saver.message}${saver.keptOnDevice ? ' Your changes are kept on this device.' : ''}`, retry: false };
				return { tone: 'pending', text: `${online ? 'Colophon isn’t answering' : 'Offline'} — ${kept}`, retry: true };
			case 'refused':
				return { tone: 'attention', text: `Not saved: ${saver.message}`, retry: true };
			case 'held':
				return { tone: 'attention', text: `Not saved: ${saver.message}`, retry: false };
			case 'signed-out':
				return { tone: 'attention', text: 'Signed out — your changes are kept on this device.', retry: false };
			default:
				return null;
		}
	});

	/** "Save now" is offered only when there is something to save: changes waiting (still shown,
	 *  disabled, while they're on their way, so it doesn't blink during an autosave), or a save that
	 *  failed and may go if tried again. Saved, a conflict and held content don't offer it. */
	const offersSave = $derived(
		saver.status === 'unsaved' || saver.status === 'saving' || saver.status === 'retrying' || saver.status === 'refused' || saver.status === 'signed-out'
	);

	onMount(() => {
		const local = kept(project, entry);
		show(local === null ? initial.draft.content : saver.restore(local, initial.draft));
		const onKey = (event: KeyboardEvent) => {
			if ((event.metaKey || event.ctrlKey) && event.key === 's') {
				event.preventDefault();
				saver.saveNow();
			}
		};
		// Edits are kept on this device, so leaving only warns when they couldn't be.
		const onLeave = (event: BeforeUnloadEvent) => {
			if (saver.status !== 'saved' && !saver.keptOnDevice) event.preventDefault();
		};
		const onLine = () => (online = navigator.onLine);
		window.addEventListener('keydown', onKey);
		window.addEventListener('beforeunload', onLeave);
		window.addEventListener('online', onLine);
		window.addEventListener('offline', onLine);
		return () => {
			window.removeEventListener('keydown', onKey);
			window.removeEventListener('beforeunload', onLeave);
			window.removeEventListener('online', onLine);
			window.removeEventListener('offline', onLine);
			saver.destroy();
			Object.values(editors).forEach((editor) => editor.destroy());
		};
	});
</script>

{#snippet version(content: Content)}
	{#each definition.fields as field (field.id)}
		<div class="grid gap-0.5 py-1.5">
			<span class="text-xs font-medium text-muted-foreground">{field.label}</span>
			{#if field.type === 'rich_text' && content[field.id] !== undefined}
				<div class="prose-colophon text-sm">
					<RichText value={content[field.id] as RichTextValue} {assetUrl}>
						{#snippet fallback(problem)}<p>This can't be shown: {problem}</p>{/snippet}
					</RichText>
				</div>
			{:else}
				<span class="text-sm">{shown(field, content[field.id])}</span>
			{/if}
		</div>
	{/each}
{/snippet}

<!-- Beside the site preview, the panel moves under the fields, so they keep their width; the
     pane says whether what it shows is saved. -->
<PreviewSplit open={previewOpen} id="entry-{entry}">
<div class="grid gap-x-10 gap-y-8 {previewOpen ? '' : 'xl:grid-cols-[minmax(0,1fr)_20rem]'}">
	<div class="min-w-0">
		<!-- The title is the page's heading, edited where it is read (D-184). -->
		{#if headingField}
			<EntryTitle
				field={headingField}
				bind:value={form[headingField.id]}
				locked={locked.includes(headingField.id)}
				refused={saver.status === 'refused' ? { pointer: saver.pointer, message: saver.message, reason: saver.reason } : null}
				onchange={changed}
			/>
		{:else}
			<h1 class="page-title mb-6" data-user-content>{latest.revision.title || 'Untitled'}</h1>
		{/if}

		{#if savedElsewhere}
			<p role="status" class="mb-6 rounded-lg bg-pending-soft px-4 py-3 text-sm text-pending-ink" data-testid="newer-version">
				A newer version was saved {clock(latest.updated_at)}: <span class="font-medium" data-user-content>“{latest.revision.title || 'Untitled'}”</span>. Your words here are kept;
				your next save asks which version to keep.
			</p>
		{/if}

		{#if unopenable}
			<p role="alert" class="mb-4 rounded-lg border border-destructive/30 bg-destructive/5 px-4 py-3 text-sm text-destructive">
				This entry can't be opened in the editor without changing it: {unopenable}
			</p>
		{/if}

		{#if saveProblem}
			<!-- One banner across the form for a save that didn't land (P3-16), with a way to try again. -->
			<div
				role="alert"
				class="mb-6 flex flex-wrap items-center gap-x-3 gap-y-1 rounded-lg px-4 py-3 text-sm {saveProblem.tone === 'pending' ? 'bg-pending-soft text-pending-ink' : 'bg-attention-soft text-attention-ink'}"
				data-testid="save-problem"
			>
				{#if saver.status === 'retrying' && !saver.busy}<WifiOffIcon class="size-4 shrink-0" aria-hidden="true" />{:else}<CloudAlertIcon class="size-4 shrink-0" aria-hidden="true" />{/if}
				<p class="min-w-0 flex-1">{saveProblem.text}</p>
				{#if saver.status === 'signed-out'}
					<a class="font-medium underline underline-offset-2" href={signInUrl(window.location.pathname)} data-sveltekit-reload>Sign in again</a>
				{:else if saveProblem.retry}
					<button
						type="button"
						class="rounded-md font-medium underline underline-offset-2 outline-none focus-visible:ring-[3px] focus-visible:ring-ring/40"
						onclick={() => saver.saveNow()}
						data-testid="retry-save">Retry now</button
					>
				{/if}
			</div>
		{/if}

		{#if saver.conflict}
			<section class="mb-6 rounded-xl border border-attention/40 bg-attention-soft/60 p-5" aria-labelledby="conflict-heading" data-testid="conflict">
				<h2 id="conflict-heading" class="font-semibold text-attention-ink">{saver.message}</h2>
				<p class="mt-1 text-sm text-attention-ink">
					<strong>Keep my version</strong> saves yours on top of theirs; theirs stays in the history.
					<strong>Use their version</strong> discards your unsaved changes.
				</p>
				<div class="mt-4 grid gap-3 md:grid-cols-2">
					<article class="rounded-lg border border-border bg-card p-4" data-testid="theirs">
						<h3 class="mb-2 text-sm font-semibold">Their version</h3>
						{@render version(saver.conflict.theirs.content)}
					</article>
					<article class="rounded-lg border border-border bg-card p-4" data-testid="mine">
						<h3 class="mb-2 text-sm font-semibold">Your version</h3>
						{@render version(saver.conflict.mine)}
					</article>
				</div>
				<div class="mt-4 flex flex-wrap gap-2">
					<Button onclick={keepMine}>Keep my version</Button>
					<Button variant="outline" onclick={useTheirs}>Use their version</Button>
				</div>
			</section>
		{/if}

		<div class="mb-6 flex flex-wrap items-center justify-between gap-3">
		<div role="tablist" aria-label="Entry" class="inline-flex rounded-lg bg-muted p-1">
			{#each tabs as [value, name] (value)}
				<button
					role="tab"
					id="tab-{value}"
					aria-selected={tab === value}
					aria-controls="panel-{value}"
					tabindex={tab === value ? 0 : -1}
					class="rounded-md px-3 py-1.5 text-sm font-medium text-muted-foreground transition-colors outline-none hover:text-foreground focus-visible:ring-[3px] focus-visible:ring-ring/40 aria-selected:bg-background aria-selected:text-foreground aria-selected:shadow-xs"
					onclick={() => (tab = value)}
					onkeydown={(event) => {
						if (event.key === 'ArrowRight' || event.key === 'ArrowLeft') {
							const at = tabs.findIndex(([v]) => v === tab);
							const next = tabs[(at + (event.key === 'ArrowRight' ? 1 : tabs.length - 1)) % tabs.length];
							if (next !== undefined) {
								tab = next[0];
								document.getElementById(`tab-${tab}`)?.focus();
							}
						}
					}}
				>
					{name}{#if value === 'history' && revisions.data}<span class="ml-1.5 text-xs text-muted-foreground tabular">{revisions.data.length}</span>{/if}
				</button>
			{/each}
		</div>
		<div class="flex flex-wrap items-center justify-end gap-2">
			<Button
				variant={commentsOpen ? 'secondary' : 'ghost'}
				size="sm"
				bind:ref={commentsButton}
				aria-expanded={commentsOpen}
				aria-controls="comments-{entry}"
				onclick={() => openComments(!commentsOpen)}
				data-testid="comments-button"
			>
				<MessageSquareIcon /><span class="max-sm:sr-only">Comments</span>{#if commentCount > 0}<span class="tabular text-muted-foreground" data-testid="comments-count">{commentCount}</span>{/if}
			</Button>
			<PreviewMenu {project} {entry} bind:open={previewOpen} manages={may(mine.data, 'destinations.manage')} />
		</div>
		</div>

		{#if restoreNote}
			<p
				role={restoreNote.kind === 'failed' ? 'alert' : 'status'}
				class="mb-4 rounded-lg px-4 py-3 text-sm {restoreNote.kind === 'failed' ? 'bg-attention-soft text-attention-ink' : 'bg-live-soft text-live-ink'}"
				data-testid="restore-note"
			>
				{restoreNote.text}
			</p>
		{/if}

		<div role="tabpanel" id="panel-content" aria-labelledby="tab-content" hidden={tab !== 'content'}>
			{#if hidden.size > 0 || locked.length > 0}
				<p class="mb-4 rounded-lg border border-border bg-card px-4 py-3 text-sm text-muted-foreground" data-testid="role-limits">
					{#if hidden.size > 0}Your role doesn’t show {labelsOf([...hidden])}; saving keeps {hidden.size === 1 ? 'it' : 'them'} as {hidden.size === 1 ? 'it is' : 'they are'}.{/if}
					{#if locked.length > 0}Your role can read but not change {labelsOf(locked)}.{/if}
				</p>
			{/if}
			{#if Object.keys(cleared).length > 0}
				<div role="status" data-testid="cleared-notes">
					<ul class="mb-2 space-y-1 text-xs text-muted-foreground">
						{#each Object.entries(cleared) as [id, note] (id)}<li>{note}</li>{/each}
					</ul>
				</div>
			{/if}
			<!-- In the definition's order (D-147): runs of fields as rows, rich text full width where it sits. -->
			{#each segments as segment, index (index)}
				{#if segment.kind === 'plain'}
					{#key showings}
						<FieldInputs
							fields={segment.fields.filter(appliesNow)}
							bind:form
							titleField={definition.title_field}
							labels={headingField ? { [headingField.id]: headingField.label } : {}}
							onchange={changed}
							{locked}
							refused={saver.status === 'refused'
								? {
										pointer: saver.pointer,
										message: saver.message,
										reason: saver.reason,
										holder:
											saver.holder === null
												? null
												: {
														title: saver.holder.title,
														href: resolve('/projects/[project]/[collection]/[entry]', { project, collection, entry: saver.holder.entry })
													}
									}
								: null}
							{project}
						/>
					{/key}
				{:else}
					{@const field = segment.field}
					<div class="py-5 first:pt-0" hidden={!appliesNow(field)} data-testid="rich-field" data-field={field.id}>
						<p class="mb-2 text-sm font-medium">{field.label}{field.required ? '' : ' (optional)'}</p>
						<div class="overflow-hidden rounded-xl border border-border bg-card focus-within:border-ring/60">
							<RichTextToolbar editor={editors[field.id] ?? null} label="Formatting: {field.label}" {project} />
							<div class="editor px-6 py-5 sm:px-8" bind:this={hosts[field.id]}></div>
						</div>
						{#if saver.status === 'refused' && saver.pointer?.startsWith(`/${field.id}/`)}
							<div class="mt-2 rounded-lg border border-destructive/30 bg-destructive/5 px-3 py-2 text-sm text-destructive" role="alert" data-testid="rich-refusal">
								<p>{richRefusal(saver.reason, saver.message)}</p>
								<TechnicalDetails class="mt-1.5"><p class="break-words">{saver.reason ?? 'refused'} at {saver.pointer}</p></TechnicalDetails>
							</div>
						{/if}
					</div>
				{/if}
			{/each}
			<!-- What links here belongs with the content: it is not repeated on the other tabs. -->
			{#if linkedFrom.length > 0}
				<section class="mt-8" aria-labelledby="linked-from-heading" data-testid="linked-from">
					<h2 id="linked-from-heading" class="mb-1 text-sm font-semibold">Linked from</h2>
					<p class="mb-3 text-xs text-muted-foreground">Entries whose fields link to this one.</p>
					<ul class="overflow-hidden rounded-xl border border-border bg-card">
						{#each linkedFrom as item (`${item.link.entry_id}-${item.link.field}`)}
							<li class="rule-dotted flex flex-wrap items-center gap-x-3 gap-y-1 px-4 py-2.5 text-sm last:border-b-0" data-testid="linked-from-entry">
								<LinkIcon class="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
								<span class="min-w-0 flex-1">
									{#if item.read === undefined}
										<span class="text-muted-foreground">Loading…</span>
									{:else if item.read.seen}
										<a
											class="font-medium hover:underline"
											data-user-content
											href={resolve('/projects/[project]/[collection]/[entry]', { project, collection: item.link.collection, entry: item.link.entry_id })}
											>{shownTitle(item.read.title)}</a
										>
									{:else}
										<span class="text-muted-foreground">An entry you can’t see</span>
									{/if}
									<span class="block text-xs text-muted-foreground" data-user-content>{item.collectionName}, in {item.fieldLabel}</span>
								</span>
								{#if item.link.published}
									<StatusBadge state="live" text="Readers see this link" testid="link-state" />
								{:else}
									<StatusBadge state="draft" text="In a draft" testid="link-state" />
								{/if}
							</li>
						{/each}
					</ul>
				</section>
			{/if}
		</div>

		{#if sitePage || siteTheme}
			<div role="tabpanel" id="panel-visual" aria-labelledby="tab-visual" hidden={tab !== 'visual'}>
				{#if siteTheme}
					<ThemeLook content={canvasContent ?? {}} />
				{:else}
					<SiteCanvas {project} content={canvasContent ?? {}} onsection={editSection} />
				{/if}
			</div>
		{/if}

		<div role="tabpanel" id="panel-history" aria-labelledby="tab-history" hidden={tab !== 'history'}>
			{#if revisions.data}
				<EntryHistory
					{project}
					{collection}
					{entry}
					{definition}
					revisions={revisions.data}
					{published}
					draft={current}
					{restores}
					ready={saver.status === 'saved'}
					{restoring}
					onrestore={restore}
				/>
			{/if}
		</div>
	</div>

	<!--
	  The panel (D-184): a status card — where it stands for readers in one line, and the save state
	  — then the one next step with ⋯ More, then the entry's language and its Details. On wide
	  screens a card beside the fields; below xl a bar that stays at the bottom of the screen while
	  the fields scroll, with the status and the next step on one row, so Publish never sits under a
	  long body (D-059, rule 7). The card's box is left out there (`contents`), so the bar sticks
	  within the whole page, and the language and Details follow the fields.
	-->
	<div class="min-w-0 max-xl:contents xl:sticky xl:top-20 xl:self-start xl:rounded-card xl:border xl:border-border xl:bg-card" data-testid="entry-panel">
		<aside
			class="sticky bottom-0 z-20 -mx-5 flex min-w-0 flex-wrap items-center gap-x-3 gap-y-2 border-t border-border bg-background/95 px-5 py-3 backdrop-blur sm:-mx-8 sm:px-8 lg:-mx-10 lg:px-10 xl:static xl:z-auto xl:m-0 xl:block xl:space-y-4 xl:border-t-0 xl:bg-transparent xl:p-4 xl:backdrop-blur-none"
			aria-label="Status and publishing"
			data-testid="entry-actions"
		>
			<!-- The status card: the badge and the save state on one row, then one sentence with the
			     release, marked by a rule in the badge's tone. -->
			<div class="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1 max-xl:flex-auto xl:grid xl:gap-2" data-testid="entry-status">
				<div class="flex min-w-0 flex-wrap items-center justify-between gap-x-3 gap-y-1">
					<StatusBadge state={shownState} generation={published?.generation ?? null} testid="entry-state" />
					<p class="flex min-w-0 items-center gap-1.5 text-sm" role="status" data-testid="save-status" data-status={saver.status}>
						{#if saver.status === 'saved'}
							<CloudCheckIcon class="pop size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
						{:else if saver.status === 'saving'}
							<LoaderIcon class="size-4 shrink-0 animate-spin text-muted-foreground" aria-hidden="true" />
						{:else if saver.status === 'unsaved'}
							<span class="mx-1 size-2 shrink-0 rounded-full bg-pending" aria-hidden="true"></span>
						{:else}
							<CloudAlertIcon class="size-4 shrink-0 text-destructive" aria-hidden="true" />
						{/if}
						<!-- Why it didn't save is in the status itself, not only in the banner above the fields: a refusal
						     names its field and reason, a busy server says how long it waits. Cut to one line here; the
						     banner and the hover say it in full. -->
						<span
							class="min-w-0 truncate {saver.status === 'conflict' || saver.status === 'refused' || saver.status === 'signed-out' || saver.status === 'held' ? 'text-destructive' : 'text-muted-foreground'}"
							title={saveReason || undefined}
							>{saveWord}{#if saveReason}<span class="text-muted-foreground" data-testid="save-reason">: {saveReason}</span>{/if}{#if saver.status === 'saved'}<span class="ml-1 tabular max-xl:hidden" title={new Date(latest.updated_at).toLocaleString()}>{clock(latest.updated_at)}</span>{/if}</span
						>
						{#if offersSave}
							<Button
								variant="link"
								size="xs"
								class="h-auto px-0 max-xl:hidden"
								onclick={() => saver.saveNow()}
								disabled={saver.status === 'saving'}
								title="Save now (⌘S)"
								aria-keyshortcuts="Meta+S Control+S"
								data-testid="save-now">Save now</Button
							>
						{/if}
					</p>
				</div>
				<p class="relative hidden pl-3 text-sm text-pretty xl:block" data-testid="publish-state">
					<span class="absolute inset-y-0.5 left-0 w-0.5 rounded-full {rule}" aria-hidden="true"></span>
					{#if published !== null}
						{@const release = resolve('/projects/[project]/releases/[generation]', { project, generation: String(published.generation) })}
						Live in <a class="font-medium underline decoration-border underline-offset-4 hover:decoration-foreground" href={release}>{releaseName(published.generation)}</a
						>{#if review !== null}<span class="text-muted-foreground">{' · '}</span><a
								class="underline decoration-border underline-offset-4 hover:decoration-foreground"
								href={resolve('/projects/[project]/changesets/[changeset]', { project, changeset: review.id })}
								>{review.runAt ? `${review.unpublish ? 'comes down' : 'changes publish'} ${runsAt}` : review.unpublish ? 'taking it down is in review' : 'changes in review'}</a
							>{:else if standing === 'changed'}<span class="text-muted-foreground">{' · edited since'}</span>{:else if liveRelease}<span class="text-muted-foreground" title={new Date(liveRelease.published_at).toLocaleString()}>{` · ${clock(liveRelease.published_at)}`}</span>{/if}
					{:else if review !== null}
						{review.runAt ? `Publishes ${runsAt}, in` : 'Waiting for review in'} <a
							class="font-medium underline decoration-border underline-offset-4 hover:decoration-foreground"
							href={resolve('/projects/[project]/changesets/[changeset]', { project, changeset: review.id })}
							data-user-content>“{review.title}”</a
						>
					{:else if takenDown}
						Readers no longer get it
					{:else}
						Not on your site yet
					{/if}
				</p>
				{#if saver.message && saver.status !== 'refused' && saver.status !== 'conflict' && saver.status !== 'held' && saver.status !== 'retrying' && saver.status !== 'signed-out'}
					<p class="basis-full text-sm text-muted-foreground" data-testid="save-message">{saver.message}</p>
				{/if}
			</div>

			<div class="max-xl:contents xl:border-t xl:border-border xl:pt-4">
				<PublishEntry
					{project}
					{collection}
					{entry}
					title={latest.revision.title}
					revision={latest.revision.id}
					mine={mine.data}
					{standing}
					{review}
					{takenDown}
					saved={saver.status === 'saved'}
					seenGeneration={releases.data?.generation}
					own={who.data != null && latest.created_by === who.data.id}
					{linkedLive}
					published={published !== null && openToAnyone}
					{duplicates}
					oncopy={() => copyAddress(publicAddress)}
					onduplicate={duplicate}
					ontechnical={showTechnical}
					onunpublished={unpublished}
				/>
			</div>
		</aside>

		<div class="min-w-0 space-y-4 text-sm max-xl:mt-2 max-xl:rounded-card max-xl:border max-xl:border-border max-xl:bg-card max-xl:p-4 xl:border-t xl:border-border xl:p-4" data-testid="entry-details">
			{#if definition.single !== true}
				<EntryLanguage
					{project}
					{collection}
					{entry}
					{definition}
					{latest}
					title={latest.revision.title}
					saved={saver.status === 'saved'}
					translates={may(mine.data, 'content.create', collection)}
					changes={mayOn(mine.data, 'content.update', collection, who.data != null && latest.created_by === who.data.id)}
					{setLocale}
				/>
			{/if}

			<!-- Details, closed until asked: when it was made and saved, the public link, the ids. -->
			<details class="group/details" bind:open={detailsOpen} data-testid="details">
				<summary
					bind:this={detailsSummary}
					class="-mx-1 inline-flex cursor-pointer list-none items-center gap-1 rounded-md px-1 text-sm text-muted-foreground outline-none select-none hover:text-foreground focus-visible:ring-[3px] focus-visible:ring-ring/40 [&::-webkit-details-marker]:hidden"
				>
					<ChevronRightIcon class="size-4 transition-transform group-open/details:rotate-90" aria-hidden="true" />Details
				</summary>
				<!-- One column that may shrink: its long one-line values (the public address, ids) truncate
				     instead of widening the page on a phone. -->
				<div class="mt-3 grid grid-cols-[minmax(0,1fr)] gap-3">
					<dl class="grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1.5">
						<dt class="text-muted-foreground">Created</dt>
						<dd class="tabular" title={new Date(latest.created_at).toLocaleString()}>{clock(latest.created_at)}</dd>
						<dt class="text-muted-foreground">Saved</dt>
						<dd class="tabular" title={new Date(latest.updated_at).toLocaleString()}>{clock(latest.updated_at)}</dd>
					</dl>
					{#if published !== null && openToAnyone}
						<p class="flex flex-wrap items-center gap-x-2 gap-y-1">
							<span class="text-muted-foreground">{standing === 'changed' ? 'Anyone can read the published version.' : 'Anyone can read it.'}</span>
							<Button variant="secondary" size="xs" onclick={() => copyAddress(publicAddress)} data-testid="copy-public-link-details"><LinkIcon data-icon="inline-start" />Copy public link</Button>
						</p>
					{:else if published !== null && gateWords !== ''}
						<p class="text-muted-foreground" data-testid="reader-gate">{gateWords}</p>
					{/if}
					<TechnicalDetails bind:open={technicalOpen}>
						{#if published !== null}
							<!-- Where readers get it (the public read, no sign-in): the published version, not the draft. -->
							{@const address = publicAddress}
							<section class="space-y-1.5 border-b border-dashed border-border pb-2.5" aria-labelledby="public-address-heading" data-testid="public-address">
								<h2 id="public-address-heading" class="font-medium text-foreground">Where readers get it</h2>
								<p>
									{openToAnyone ? 'Anyone can read the published version at this address, without signing in' : 'Readers signed in on your site with access read the published version at this address; anyone else is told it isn’t there'}{standing ===
									'changed'
										? `: the one live in ${releaseName(published.generation)}, not your newer changes`
										: ''}.
								</p>
								<div class="flex min-w-0 items-center gap-1 rounded-lg border border-border bg-card px-2.5 py-1.5">
									<a class="min-w-0 flex-1 truncate font-mono underline underline-offset-2" href={address} target="_blank" rel="noreferrer"
										>{address}<ArrowUpRightIcon class="ml-0.5 inline size-3" aria-hidden="true" /></a
									>
									<Button variant="ghost" size="icon-xs" aria-label="Copy the public address" onclick={() => copyAddress(address)}><CopyIcon /></Button>
								</div>
								<Button variant="outline" size="xs" onclick={() => (readerView = true)} data-testid="see-what-readers-get"
									><EyeIcon data-icon="inline-start" />See what readers get</Button
								>
							</section>
						{/if}
						<p>Entry <span class="font-mono break-all">{entry}</span> in collection <span class="font-mono">{collection}</span></p>
						<p>Revision <span class="font-mono break-all">{latest.revision.id}</span></p>
						<p>Content hash <span class="font-mono break-all">{latest.revision.content_hash}</span></p>
						{#if latest.locale}<p>Locale <span class="font-mono">{latest.locale}</span></p>{/if}
						<p>Readers get an entry only once a release with it is published.</p>
					</TechnicalDetails>
				</div>
			</details>
		</div>
	</div>
	{#if published !== null}
		<ReaderView bind:open={readerView} {project} {collection} {entry} changed={standing === 'changed'} titleLabel={titleLabel(definition)} />
	{/if}
</div>
	{#snippet pane()}
		<PreviewPane
			{project}
			{entry}
			title={latest.revision.title}
			status={saver.status}
			saved={acknowledged}
			manages={may(mine.data, 'destinations.manage')}
			onclose={() => (previewOpen = false)}
		/>
	{/snippet}
</PreviewSplit>

<!-- Escape closes the drawer wherever the focus is — after posting, the focus may have left it —
     unless a menu or a modal dialog is open, whose own Escape comes first. -->
<svelte:window
	onkeydown={(event) => {
		if (event.key !== 'Escape' || !commentsOpen || event.defaultPrevented) return;
		if (document.querySelector('[role="menu"], [role="alertdialog"], [role="dialog"][aria-modal="true"]') !== null) return;
		void openComments(false);
	}}
/>

<!-- Comments beside the content (D-184): a drawer that leaves the text in view. Kept mounted, so
     the count on its button is right before it is opened. -->
<div
	id="comments-{entry}"
	role="dialog"
	aria-modal="false"
	aria-labelledby="comments-heading-{entry}"
	tabindex="-1"
	class="fixed inset-y-0 right-0 z-30 flex w-full flex-col border-l border-border bg-popover shadow-float sm:w-[22.5rem]"
	hidden={!commentsOpen}
	onkeydown={(event) => {
		if (event.key === 'Escape') {
			event.stopPropagation();
			void openComments(false);
		}
	}}
	data-testid="comments-drawer"
>
	<div class="flex h-14 shrink-0 items-center justify-between gap-2 border-b border-border px-4">
		<h2 id="comments-heading-{entry}" class="text-base font-medium outline-none" tabindex="-1" bind:this={commentsHeading}>Comments</h2>
		<Button variant="ghost" size="icon-sm" aria-label="Close comments" onclick={() => openComments(false)} data-testid="comments-close"><XIcon /></Button>
	</div>
	<div class="min-h-0 flex-1 overflow-y-auto overscroll-contain p-4">
		<!-- Comments (E1, D-102): each one binds the version on screen — the draft's current
		     version — so what was said stays bound to exactly what its author saw. -->
		<CommentsPanel {project} {collection} {entry} revision={latest.revision.id} canWrite={commentWrite} oncount={(open) => (commentCount = open)} />
	</div>
</div>
