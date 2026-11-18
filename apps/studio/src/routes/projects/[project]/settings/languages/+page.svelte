<!--
  Settings → Languages (gap/locales): the languages this environment's entries are written in, in
  display order, one of them the default, each with the languages readers get instead, in order,
  where an entry isn't written in it ("If missing, show"). With none, the project publishes in one
  language and an entry may carry any language code, the way it always worked. Saving replaces the
  whole list on the etag it was read at: a list someone changed meanwhile is refused
  `stale_locales` and the latest one loads; removing a language entries are still written in is
  refused `locale_in_use`, and the page names those entries, linking each one it can find. Any
  member may read the list, so it shows read-only to a role that can't change it.
-->
<script lang="ts">
	import ArrowDownIcon from '@lucide/svelte/icons/arrow-down';
	import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';
	import CircleCheckIcon from '@lucide/svelte/icons/circle-check';
	import LanguagesIcon from '@lucide/svelte/icons/languages';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import XIcon from '@lucide/svelte/icons/x';
	import { tick } from 'svelte';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { toast } from 'svelte-sonner';
	import * as AlertDialog from '@colophon/ui/ui/alert-dialog';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button, buttonVariants } from '@colophon/ui/ui/button';
	import * as Dialog from '@colophon/ui/ui/dialog';
	import * as DropdownMenu from '@colophon/ui/ui/dropdown-menu';
	import { Input } from '@colophon/ui/ui/input';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { ApiError, isLocaleTag, type Locale, type Locales, MAX_FALLBACKS, MAX_LOCALES, may, newKey, replaceLocales, searchEntries } from '#lib/api.ts';
	import { plainError } from '#lib/errors.ts';
	import { count, localesQuery, projectQuery, typesQuery } from '#lib/queries.ts';
	import SaveBar from '#lib/SaveBar.svelte';
	import SettingsFrame from '#lib/SettingsFrame.svelte';
	import EmptyState from '#lib/shell/EmptyState.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';
	import TechnicalDetails from '#lib/shell/TechnicalDetails.svelte';
	import { shownTitle } from '#lib/words.ts';

	const project = $derived(page.params.project ?? '');
	const queryClient = useQueryClient();
	const mine = createQuery(() => projectQuery(project));
	const locales = createQuery(() => localesQuery(project));
	const changes = $derived(may(mine.data, 'schema.change'));

	const copy = (list: Locale[]): Locale[] => list.map(({ tag, label, default: isDefault, fallback }) => ({ tag, label, default: isDefault, fallback: [...fallback] }));
	/** What a save sends: the list as edited, names trimmed, each language's fields in one order,
	 *  so comparing two lists as text is exact. */
	const body = (list: Locale[]): Locale[] =>
		list.map(({ tag, label, default: isDefault, fallback }) => ({ tag, label: label.trim(), default: isDefault, fallback: [...fallback] }));

	/** The list the edits started from: the etag a save names, and what Discard goes back to. */
	let base = $state.raw<Locales | null>(null);
	let draft = $state<Locale[]>([]);
	const dirty = $derived(base !== null && JSON.stringify(body(draft)) !== JSON.stringify(body(base.locales)));
	function seed(from: Locales) {
		base = from;
		draft = copy(from.locales);
	}
	// Seed when the list first reads, and follow a newer one (someone else saved, and a live hint
	// refetched it) while nothing here is edited: an unsaved edit is never clobbered, and a save
	// always names the list its edits were made on.
	$effect(() => {
		const latest = locales.data;
		if (latest !== undefined && (base === null || (latest.version !== base.version && !dirty))) seed(latest);
	});

	/** What people call a language: its name in this list, else in the saved one, else its code. */
	const labelOf = (tag: string) => draft.find((row) => row.tag === tag)?.label.trim() || base?.locales.find((row) => row.tag === tag)?.label || tag;
	const savedLabel = (tag: string) => base?.locales.find((row) => row.tag === tag)?.label || tag;

	// What keeps a save from going, before it is sent: the server checks all of it again.
	const problems = $derived.by(() => {
		const found: string[] = [];
		for (const row of draft) {
			if (row.label.trim() === '') found.push(`Give ${row.tag} a name.`);
			else if (row.label.trim().length > 80) found.push(`The name of ${row.tag} is longer than 80 characters.`);
		}
		if (draft.length > MAX_LOCALES) found.push(`A project lists at most ${MAX_LOCALES} languages.`);
		if (draft.length > 0 && draft.filter((row) => row.default).length !== 1) found.push('Choose one language as the default.');
		return found;
	});

	let busy = $state(false);
	/** A refusal said for the whole list. */
	let error = $state('');
	/** The list moved on meanwhile: the latest one is shown, the edits made here are gone. */
	let stale = $state(false);
	/** A refusal the server placed in the list: the language (by its code, as sent) and the part. */
	let placed = $state<{ tag: string; part: 'tag' | 'label' | 'fallback' | null; message: string } | null>(null);
	let justSaved = $state(false);
	$effect(() => {
		if (dirty) justSaved = false;
	});
	/** An edit: a refusal placed in the list no longer describes it. */
	function touched() {
		placed = null;
	}

	function makeDefault(index: number) {
		draft.forEach((row, at) => (row.default = at === index));
		touched();
	}

	// "If missing, show": at most four other languages, each once, in order.
	function focusOn(selector: string, fallback?: string) {
		const found = document.querySelector<HTMLElement>(selector) ?? (fallback === undefined ? null : document.querySelector<HTMLElement>(fallback));
		found?.focus();
	}
	const moveSelector = (row: string, tag: string, way: 'up' | 'down') => `[data-move="${CSS.escape(`${row}|${tag}|${way}`)}"]`;
	async function addFallback(index: number, tag: string) {
		const row = draft[index];
		if (row === undefined || row.tag === tag || row.fallback.includes(tag) || row.fallback.length >= MAX_FALLBACKS) return;
		row.fallback.push(tag);
		touched();
		// The menu gives focus back to Add; once nothing is left to add, Add is gone, and focus
		// goes to the language just added instead.
		await tick();
		if (document.querySelector(`[data-add-fallback="${CSS.escape(row.tag)}"]`) === null) {
			focusOn(`[data-remove="${CSS.escape(`${row.tag}|${tag}`)}"]`, '[data-testid="add-language"]');
		}
	}
	async function moveFallback(index: number, at: number, by: -1 | 1) {
		const row = draft[index];
		const to = at + by;
		if (row === undefined || to < 0 || to >= row.fallback.length) return;
		const [tag] = row.fallback.splice(at, 1);
		if (tag === undefined) return;
		row.fallback.splice(to, 0, tag);
		touched();
		// Keyboard use stays on the language just moved: the same button while it can still move
		// that way, else the other one.
		await tick();
		const last = row.fallback.length - 1;
		const way = by === -1 ? (to === 0 ? 'down' : 'up') : to === last ? 'up' : 'down';
		focusOn(moveSelector(row.tag, tag, way));
	}
	async function dropFallback(index: number, at: number) {
		const row = draft[index];
		if (row === undefined) return;
		row.fallback.splice(at, 1);
		touched();
		// Focus goes to the language that took its place, else the one before, else Add.
		await tick();
		const next = row.fallback[at] ?? row.fallback[at - 1];
		focusOn(
			next === undefined ? `[data-add-fallback="${CSS.escape(row.tag)}"]` : `[data-remove="${CSS.escape(`${row.tag}|${next}`)}"]`,
			'[data-testid="add-language"]'
		);
	}

	// Removing a language: asked first, then gone from the list and from every other language's
	// "If missing, show"; the default passes to the first one left. Nothing is saved until Save.
	let confirming = $state(false);
	let removing = $state<{ tag: string; label: string; next: string | null } | null>(null);
	function askRemove(index: number) {
		const row = draft[index];
		if (row === undefined) return;
		const next = row.default ? (draft.find((other) => other.tag !== row.tag) ?? null) : null;
		removing = { tag: row.tag, label: row.label.trim() || row.tag, next: next === null ? null : next.label.trim() || next.tag };
		confirming = true;
	}
	let removedAt = -1;
	function remove() {
		const gone = removing;
		confirming = false;
		if (gone === null) return;
		removedAt = draft.findIndex((row) => row.tag === gone.tag);
		const wasDefault = draft[removedAt]?.default === true;
		const next = draft.filter((row) => row.tag !== gone.tag);
		for (const row of next) row.fallback = row.fallback.filter((tag) => tag !== gone.tag);
		if (wasDefault && next[0] !== undefined) next[0].default = true;
		draft = next;
		touched();
	}
	/** Where focus goes once the question closes: the language now in that place, else Add. */
	function afterRemoveQuestion(event: Event) {
		if (removedAt < 0) return;
		event.preventDefault();
		const row = draft[Math.min(removedAt, draft.length - 1)];
		removedAt = -1;
		focusOn(row === undefined ? '[data-testid="add-language"]' : `[data-remove-language="${CSS.escape(row.tag)}"]`, '[data-testid="add-language"]');
	}

	// Adding a language: its code, checked here with the server's rule, and its name, suggested
	// from the code in that language itself ("pt-BR" → "Português (Brasil)") until typed over.
	let adding = $state(false);
	let newTag = $state('');
	let newLabel = $state('');
	let labelTyped = $state(false);
	// Problems show once Add is pressed: shown as the field is left, they moved the button out
	// from under the pointer between press and release, and the click never landed.
	let tried = $state(false);
	function nameFor(tag: string): string {
		if (!isLocaleTag(tag)) return '';
		try {
			const name = new Intl.DisplayNames([tag], { type: 'language' }).of(tag);
			if (name === undefined || name.toLowerCase() === tag.toLowerCase()) return '';
			return name.slice(0, 1).toLocaleUpperCase(tag) + name.slice(1);
		} catch {
			return '';
		}
	}
	const shownLabel = $derived(labelTyped ? newLabel : nameFor(newTag.trim()));
	const tagProblem = $derived.by(() => {
		const tag = newTag.trim();
		if (tag === '') return 'Enter a language code, like en, fr or pt-BR.';
		if (!isLocaleTag(tag)) return 'A language code is two or three letters, then parts of letters or digits after hyphens, like en, fr or pt-BR.';
		const taken = draft.find((row) => row.tag.toLowerCase() === tag.toLowerCase());
		if (taken !== undefined) return `${taken.label.trim() || taken.tag} is in the list already.`;
		return '';
	});
	const labelProblem = $derived(
		shownLabel.trim() === '' ? 'Give it the name people know it by.' : shownLabel.trim().length > 80 ? 'A name is at most 80 characters.' : ''
	);
	const showTagProblem = $derived(tagProblem !== '' && tried);
	const showLabelProblem = $derived(labelProblem !== '' && tried);
	const defaultRow = $derived(draft.find((row) => row.default));
	function startAdding(tag = '') {
		newTag = tag;
		newLabel = '';
		labelTyped = false;
		tried = false;
		adding = true;
	}
	function addLanguage(event: SubmitEvent) {
		event.preventDefault();
		tried = true;
		if (tagProblem !== '') return focusOn('#language-tag');
		if (labelProblem !== '') return focusOn('#language-name');
		if (draft.length >= MAX_LOCALES) return;
		const first = draft.length === 0;
		// A new language shows the default where it is missing, until someone says otherwise.
		const fallback = defaultRow === undefined ? [] : [defaultRow.tag];
		const tag = newTag.trim();
		draft.push({ tag, label: shownLabel.trim(), default: first, fallback });
		touched();
		// Added as the refusal asked: the list no longer leaves it out.
		if (inUse?.tags.some((other) => other.toLowerCase() === tag.toLowerCase())) inUse = null;
		adding = false;
	}

	// Saving: the whole list on the etag of the one the edits started from. One key per request
	// sent: pressing Save again after a lost answer is safe; a different list makes a new one.
	let saveKey = newKey();
	let keyFor = '';

	/** The entries in the way of a list (`locale_in_use`): a non-empty list must name every tag
	 *  live entries carry — one just removed, one tagged before there was any list, or one spelled
	 *  otherwise ("pt-br" for "pt-BR"). Each is linked when the page finds its collection by
	 *  asking each one for entries in that language. */
	interface InUse {
		total: number;
		/** The tags entries are written in that the list leaves out: the one the server names, else
		 *  the ones just removed. */
		tags: string[];
		entries: { entry: string; title: string; href: string | null }[];
		/** Each collection holding some, with its list filtered to that language. */
		places: { name: string; tag: string; href: string; count: string }[];
		finding: boolean;
	}
	let inUse = $state<InUse | null>(null);

	async function findInUse(failure: ApiError, removed: string[]) {
		const listed = failure.details?.entries ?? [];
		const total = failure.details?.total ?? listed.length;
		const unlinked = listed.map((item) => ({ entry: item.entry, title: item.title, href: null }));
		// The refusal names the tag in its details (D-180).
		const named = failure.details?.locale;
		const asked = named === undefined ? removed : [named];
		inUse = { total, tags: asked, entries: unlinked, places: [], finding: true };
		try {
			const types = await queryClient.fetchQuery(typesQuery(project));
			const asks = types.flatMap((type) => asked.map((tag) => ({ type, tag })));
			const answers = await Promise.allSettled(asks.map(async ({ type, tag }) => ({ type, tag, found: await searchEntries(project, type.slug, '', null, null, tag) })));
			const where = new Map<string, string>();
			const places: InUse['places'] = [];
			for (const answer of answers) {
				if (answer.status !== 'fulfilled' || answer.value.found.data.length === 0) continue;
				const { type, tag, found } = answer.value;
				for (const row of found.data) where.set(row.id, type.slug);
				places.push({
					name: type.definition.name,
					tag,
					href: `${resolve('/projects/[project]/[collection]', { project, collection: type.slug })}?locale=${encodeURIComponent(tag)}`,
					count: count(found)
				});
			}
			const tags = asked.filter((tag) => places.some((place) => place.tag === tag));
			inUse = {
				total,
				tags: tags.length > 0 ? tags : asked,
				entries: listed.map((item) => {
					const collection = where.get(item.entry);
					return {
						entry: item.entry,
						title: item.title,
						href: collection === undefined ? null : resolve('/projects/[project]/[collection]/[entry]', { project, collection, entry: item.entry })
					};
				}),
				places,
				finding: false
			};
		} catch {
			inUse = { total, tags: asked, entries: unlinked, places: [], finding: false };
		}
	}
	/** Of the tags in the way: ones the saved list has (Keep puts them back), ones no language
	 *  in the list matches in any spelling (Add offers them), and ones the list spells otherwise. */
	const keepable = $derived((inUse?.tags ?? []).filter((tag) => base?.locales.some((row) => row.tag === tag) && !draft.some((row) => row.tag === tag)));
	const addable = $derived(
		(inUse?.tags ?? []).filter((tag) => !keepable.includes(tag) && !draft.some((row) => row.tag.toLowerCase() === tag.toLowerCase()))
	);
	const respelled = $derived(
		(inUse?.tags ?? []).flatMap((tag) => {
			const row = draft.find((one) => one.tag !== tag && one.tag.toLowerCase() === tag.toLowerCase());
			return row === undefined ? [] : [{ tag, as: row }];
		})
	);
	/** A language's name where the saved list or the edits know it, for a tag in the way. */
	const knownLabel = (tag: string) => base?.locales.find((row) => row.tag === tag)?.label ?? draft.find((row) => row.tag === tag)?.label.trim();

	/** Puts removed languages back where they were, keeping every other edit. */
	function keep(tags: string[]) {
		if (base === null) return;
		for (const tag of tags) {
			const at = base.locales.findIndex((row) => row.tag === tag);
			const saved = base.locales[at];
			if (saved === undefined || draft.some((row) => row.tag === tag)) continue;
			if (saved.default) for (const row of draft) row.default = false;
			draft.splice(Math.min(at, draft.length), 0, { ...saved, fallback: saved.fallback.filter((other) => draft.some((row) => row.tag === other)) });
		}
		inUse = null;
		touched();
	}

	async function save() {
		if (base === null || busy) return;
		const sent = body(draft);
		const fingerprint = `${base.etag}\n${JSON.stringify(sent)}`;
		if (fingerprint !== keyFor) {
			saveKey = newKey();
			keyFor = fingerprint;
		}
		const removed = base.locales.map((row) => row.tag).filter((tag) => !sent.some((row) => row.tag === tag));
		busy = true;
		error = '';
		stale = false;
		placed = null;
		inUse = null;
		try {
			const saved = await replaceLocales(project, sent, base.etag, saveKey);
			keyFor = '';
			queryClient.setQueryData(['locales', project], saved);
			seed(saved);
			justSaved = true;
			toast.success('Saved the languages');
		} catch (failure) {
			if (failure instanceof ApiError && failure.reason === 'stale_locales') {
				// Someone else's list stands; show it, rather than save over it.
				stale = true;
				try {
					seed(await queryClient.fetchQuery({ ...localesQuery(project), staleTime: 0 }));
				} catch (again) {
					error = plainError(again, 'see the latest languages');
				}
			} else if (failure instanceof ApiError && failure.reason === 'locale_in_use') {
				void findInUse(failure, removed);
			} else {
				const at = failure instanceof ApiError && failure.status === 422 ? /^\/locales\/(\d+)(?:\/(tag|label|fallback))?/.exec(failure.details?.pointer ?? '') : null;
				const row = at === null ? undefined : sent[Number(at[1])];
				const message = plainError(failure, 'change the languages');
				if (at !== null && row !== undefined) placed = { tag: row.tag, part: (at[2] as 'tag' | 'label' | 'fallback' | undefined) ?? null, message };
				else error = message;
			}
		} finally {
			busy = false;
		}
	}

	function discard() {
		if (base !== null) seed(base);
		error = '';
		stale = false;
		placed = null;
		inUse = null;
	}
</script>

<svelte:head><title>Languages · Colophon Studio</title></svelte:head>

<SettingsFrame {project} current="languages">
	<PageHeader title="Languages">
		{#snippet description()}
			The languages your entries are written in. Readers ask for one; where a translation is missing, they get the next language under <em>If missing, show</em>, in order.
		{/snippet}
	</PageHeader>

	{#if mine.isError}
		<Problem error={mine.error} what="this project" retry={() => mine.refetch()} />
	{:else if locales.isError}
		<Problem error={locales.error} what="the languages" retry={() => locales.refetch()} />
	{:else if mine.isPending || base === null}
		<div class="max-w-3xl space-y-3" data-testid="languages-loading">
			<p class="sr-only" role="status">Loading the languages…</p>
			<Skeleton class="h-32 w-full rounded-xl" />
			<Skeleton class="h-32 w-full rounded-xl" />
		</div>
	{:else if !changes}
		<!-- Every member may read the list; only a schema.change role may replace it. -->
		<p class="max-w-3xl rounded-lg border border-border bg-card px-4 py-3 text-sm text-muted-foreground" data-testid="languages-read-only">
			Your role can't change the languages. These are the languages entries are written in.
		</p>
		{#if base.locales.length === 0}
			<p class="mt-4 max-w-3xl text-sm text-muted-foreground" data-testid="languages-none">Your project publishes in one language.</p>
		{:else}
			<ol class="mt-4 max-w-3xl overflow-hidden rounded-xl border border-border bg-card" data-testid="languages">
				{#each base.locales as row (row.tag)}
					<li class="rule-dotted grid gap-1 px-4 py-3 text-sm last:border-b-0" data-testid="language">
						<div class="flex flex-wrap items-center gap-2">
							<span class="font-medium" data-user-content>{row.label}</span>
							<span class="font-mono text-xs text-muted-foreground"><span class="sr-only">{'Code '}</span>{row.tag}</span>
							{#if row.default}<Badge variant="brand">Default</Badge>{/if}
						</div>
						<p class="text-muted-foreground">
							{row.fallback.length === 0 ? 'If missing, show nothing else.' : `If missing, show ${row.fallback.map(savedLabel).join(', then ')}.`}
						</p>
					</li>
				{/each}
			</ol>
		{/if}
	{:else if base.locales.length === 0 && draft.length === 0}
		<div class="max-w-3xl">
			<EmptyState icon={LanguagesIcon} title="Your project publishes in one language." testid="languages-empty">
				Add a language to publish translations.
				{#snippet action()}
					<Button onclick={() => startAdding()} data-testid="add-language"><PlusIcon data-icon="inline-start" />Add language</Button>
				{/snippet}
			</EmptyState>
		</div>
	{:else}
		<section class="max-w-3xl" aria-labelledby="languages-heading">
			<h2 id="languages-heading" class="sr-only">Your languages</h2>
			{#if draft.length === 0}
				<p class="rounded-xl border border-dashed border-border px-4 py-6 text-center text-sm text-muted-foreground" data-testid="languages-cleared">
					No languages. Saving goes back to one language, and an entry may carry any language code again.
				</p>
			{:else}
				<ol class="space-y-3" data-testid="languages">
					{#each draft as row, index (row.tag)}
						{@const name = row.label.trim() || row.tag}
						{@const others = draft.filter((other) => other.tag !== row.tag && !row.fallback.includes(other.tag))}
						{@const problem = placed?.tag === row.tag ? placed : null}
						<li class="rounded-xl border border-border bg-card px-4 py-3 shadow-xs" data-testid="language" data-tag={row.tag}>
							<div class="flex flex-col gap-2 sm:flex-row sm:items-center sm:gap-3">
								<div class="min-w-0 sm:flex-1">
									<label class="sr-only" for="language-label-{index}">Name of the language with the code {row.tag}</label>
									<Input
										id="language-label-{index}"
										bind:value={row.label}
										oninput={touched}
										maxlength={80}
										required
										aria-invalid={row.label.trim() === '' || problem?.part === 'label' || undefined}
										aria-describedby={problem === null ? undefined : `language-problem-${index}`}
										data-testid="language-label"
									/>
								</div>
								<div class="flex items-center gap-3">
									<span class="font-mono text-xs text-muted-foreground" data-testid="language-tag"><span class="sr-only">{'Code '}</span>{row.tag}</span>
									{#if row.default}
										<Badge variant="brand" data-testid="language-default">Default</Badge>
									{:else}
										<Button variant="outline" size="xs" onclick={() => makeDefault(index)} disabled={busy} data-testid="make-default"
											>Make default<span class="sr-only">: {name}</span></Button
										>
									{/if}
									<Button
										class="ml-auto"
										variant="ghost"
										size="icon-sm"
										onclick={() => askRemove(index)}
										disabled={busy}
										aria-label="Remove {name}"
										data-remove-language={row.tag}
										data-testid="remove-language"><XIcon /></Button
									>
								</div>
							</div>

							<div class="mt-3 grid gap-2 border-t border-dotted border-border pt-3 sm:grid-cols-[8.5rem_minmax(0,1fr)]">
								<p class="text-sm text-muted-foreground sm:pt-1" id="fallback-label-{index}">If missing, show:</p>
								<div class="min-w-0 space-y-2">
									{#if row.fallback.length === 0}
										<p class="text-sm text-muted-foreground sm:pt-1" data-testid="no-fallback">
											{draft.length === 1 ? 'Add another language to choose one here.' : `Nothing else: readers asking for ${name} get only what is written in it.`}
										</p>
									{:else}
										<ol class="flex flex-col items-start gap-1.5" aria-labelledby="fallback-label-{index}" data-testid="fallbacks">
											{#each row.fallback as tag, at (tag)}
												{@const shown = labelOf(tag)}
												<li class="flex max-w-full items-center gap-1 rounded-full border border-border bg-background py-0.5 pr-0.5 pl-3 text-sm" data-testid="fallback" data-tag={tag}>
													<span class="text-xs text-muted-foreground tabular">{at + 1}.</span>
													<span class="min-w-0 truncate">{shown}</span>
													<span class="font-mono text-xs text-muted-foreground">{tag}</span>
													<Button
														variant="ghost"
														size="icon-xs"
														onclick={() => moveFallback(index, at, -1)}
														disabled={busy || at === 0}
														aria-label="Move {shown} up for {name}"
														data-move="{row.tag}|{tag}|up"
														data-testid="fallback-up"><ArrowUpIcon /></Button
													>
													<Button
														variant="ghost"
														size="icon-xs"
														onclick={() => moveFallback(index, at, 1)}
														disabled={busy || at === row.fallback.length - 1}
														aria-label="Move {shown} down for {name}"
														data-move="{row.tag}|{tag}|down"
														data-testid="fallback-down"><ArrowDownIcon /></Button
													>
													<Button
														variant="ghost"
														size="icon-xs"
														onclick={() => dropFallback(index, at)}
														disabled={busy}
														aria-label="Stop showing {shown} for {name}"
														data-remove="{row.tag}|{tag}"
														data-testid="fallback-remove"><XIcon /></Button
													>
												</li>
											{/each}
										</ol>
									{/if}
									{#if row.fallback.length >= MAX_FALLBACKS}
										<p class="text-xs text-muted-foreground">At most four.</p>
									{:else if others.length > 0}
										<DropdownMenu.Root>
											<DropdownMenu.Trigger class={buttonVariants({ variant: 'outline', size: 'xs' })} disabled={busy} data-add-fallback={row.tag} data-testid="add-fallback">
												<PlusIcon data-icon="inline-start" />Add<span class="sr-only">: a language to show when {name} is missing</span>
											</DropdownMenu.Trigger>
											<DropdownMenu.Content align="start" class="w-64">
												{#each others as other (other.tag)}
													<DropdownMenu.Item onSelect={() => addFallback(index, other.tag)} data-testid="fallback-option">
														<span class="min-w-0 truncate">{other.label.trim() || other.tag}</span>
														<span class="ml-auto font-mono text-xs text-muted-foreground">{other.tag}</span>
													</DropdownMenu.Item>
												{/each}
											</DropdownMenu.Content>
										</DropdownMenu.Root>
									{/if}
								</div>
							</div>
							{#if problem !== null}
								<p id="language-problem-{index}" class="mt-2 text-sm text-destructive" role="alert" data-testid="language-problem">{problem.message}</p>
							{/if}
						</li>
					{/each}
				</ol>
			{/if}

			<div class="mt-4 flex flex-wrap items-center gap-3">
				<Button variant="outline" size="sm" onclick={() => startAdding()} disabled={busy || draft.length >= MAX_LOCALES} data-testid="add-language"
					><PlusIcon data-icon="inline-start" />Add language</Button
				>
				{#if draft.length >= MAX_LOCALES}<span class="text-xs text-muted-foreground">A project lists at most {MAX_LOCALES} languages.</span>{/if}
			</div>

			{#if stale}
				<div role="alert" class="mt-4 rounded-lg bg-pending-soft px-4 py-3 text-sm text-pending-ink" data-testid="languages-stale">
					<p class="font-medium">Someone changed languages; review and save again.</p>
					<p class="mt-1">This is the list as it is now. Your changes weren’t saved: make the ones you still want, then save.</p>
				</div>
			{/if}
			{#if error}
				<p role="alert" class="mt-4 rounded-lg bg-attention-soft px-4 py-3 text-sm text-attention-ink" data-testid="languages-error">{error}</p>
			{/if}
			{#if inUse !== null}
				<div role="alert" class="mt-4 rounded-lg bg-attention-soft px-4 py-3 text-sm text-attention-ink" data-testid="languages-in-use">
					<p class="font-medium" data-testid="in-use-summary">
						{inUse.total.toLocaleString()}
						{inUse.total === 1 ? 'entry is' : 'entries are'} written in
						{#each inUse.tags as tag, at (tag)}{at > 0 ? (at === inUse.tags.length - 1 ? ' or ' : ', ') : ''}{#if knownLabel(tag)}{knownLabel(tag)} (<span
									class="font-mono">{tag}</span
								>){:else}<span class="font-mono">{tag}</span>{/if}{/each}. Move or delete them first.
					</p>
					<p class="mt-1">
						{#if respelled.length > 0}
							{#each respelled as { tag, as } (tag)}They spell it <span class="font-mono">{tag}</span>; choosing {as.label.trim() || as.tag} for each one saves it as
								<span class="font-mono">{as.tag}</span>.
							{/each}
						{:else}
							Open each one and choose another language for it, or delete it, then save again.
						{/if}
					</p>
					{#if inUse.entries.length > 0}
						<ul class="mt-2 list-disc space-y-0.5 pl-5" data-testid="in-use-entries">
							{#each inUse.entries as item (item.entry)}
								<li>
									{#if item.href !== null}
										<a class="font-medium underline underline-offset-2" href={item.href} data-user-content>{shownTitle(item.title)}</a>
									{:else}
										<span data-user-content>{shownTitle(item.title)}</span>
									{/if}
								</li>
							{/each}
						</ul>
					{/if}
					{#if inUse.total > inUse.entries.length}<p class="mt-1">And {(inUse.total - inUse.entries.length).toLocaleString()} more.</p>{/if}
					{#if inUse.finding}
						<p class="mt-2" role="status">Finding where they are…</p>
					{:else if inUse.places.length > 0}
						<p class="mt-2">
							Find them all:
							{#each inUse.places as place, at (`${place.name}|${place.tag}`)}{at > 0 ? ', ' : ''}<a class="underline underline-offset-2" href={place.href} data-testid="in-use-place"
									>{place.name} in {knownLabel(place.tag) ?? place.tag} ({place.count})</a
								>{/each}.
						</p>
					{/if}
					{#if keepable.length > 0 || addable.length > 0}
						<div class="mt-3 flex flex-wrap gap-2">
							{#if keepable.length > 0}
								<Button variant="outline" size="xs" onclick={() => keep(keepable)} data-testid="keep-language">
									Keep {keepable.map((tag) => knownLabel(tag) ?? tag).join(' and ')}
								</Button>
							{/if}
							{#each addable as tag (tag)}
								<Button variant="outline" size="xs" onclick={() => startAdding(tag)} disabled={draft.length >= MAX_LOCALES} data-testid="add-in-use-language">
									<PlusIcon data-icon="inline-start" />Add <span class="font-mono">{tag}</span> to the list
								</Button>
							{/each}
						</div>
					{/if}
				</div>
			{/if}
			<div role="status" class="mt-4 text-sm" data-testid="languages-saved">
				{#if justSaved && !dirty}
					<p class="flex items-center gap-2"><CircleCheckIcon class="size-4 shrink-0 text-live" aria-hidden="true" />Saved. Entries and readers use these languages now.</p>
				{/if}
			</div>
			<SaveBar changed={dirty} {busy} {problems} what="the languages" onsave={save} ondiscard={discard} />
		</section>
	{/if}

	<TechnicalDetails class="mt-8 max-w-3xl">
		<p>
			The API calls these the environment's <em>locales</em>, named by BCP-47 tags. GET answers the list with an ETag (<code>"locales-&lt;version&gt;"</code>); PUT replaces it whole
			under <code>schema.change</code> with <code>If-Match</code> and an idempotency key. Reads take <code>?locale=</code> and <code>?fallback=policy|none|tag,tag</code>; the
			default policy is each language's <em>If missing, show</em> list.
		</p>
		<p class="font-mono break-all">/api/v1/projects/{project}/environments/production/locales</p>
	</TechnicalDetails>
</SettingsFrame>

<Dialog.Root bind:open={adding}>
	<Dialog.Content class="sm:max-w-md">
		<form class="grid gap-5" onsubmit={addLanguage} aria-label="Add language" novalidate>
			<Dialog.Header>
				<Dialog.Title>Add language</Dialog.Title>
				<Dialog.Description>
					{#if draft.length === 0}
						The first language is the default: entries with no language set count as it.
					{:else}
						Where an entry isn’t written in it yet, readers get {defaultRow === undefined ? 'nothing else' : (defaultRow.label.trim() || defaultRow.tag)} until you change that.
					{/if}
				</Dialog.Description>
			</Dialog.Header>
			<div class="grid gap-1.5 text-sm">
				<label class="font-medium" for="language-tag">Language code</label>
				<!-- svelte-ignore a11y_autofocus -->
				<Input
					id="language-tag"
					class="font-mono"
					bind:value={newTag}
					autofocus
					autocomplete="off"
					autocapitalize="off"
					spellcheck={false}
					maxlength={35}
					placeholder="pt-BR"
					aria-invalid={showTagProblem || undefined}
					aria-describedby="language-tag-hint{showTagProblem ? ' language-tag-problem' : ''}"
					data-testid="language-tag-input"
				/>
				<span id="language-tag-hint" class="text-xs text-muted-foreground">The code websites and browsers use, like en, fr or pt-BR.</span>
				{#if showTagProblem}<span id="language-tag-problem" class="text-sm text-destructive" data-testid="language-tag-problem">{tagProblem}</span>{/if}
			</div>
			<div class="grid gap-1.5 text-sm">
				<label class="font-medium" for="language-name">Name</label>
				<Input
					id="language-name"
					value={shownLabel}
					oninput={(event) => {
						labelTyped = true;
						newLabel = event.currentTarget.value;
					}}
					maxlength={80}
					placeholder="Português (Brasil)"
					aria-invalid={showLabelProblem || undefined}
					aria-describedby="language-name-hint{showLabelProblem ? ' language-name-problem' : ''}"
					data-testid="language-name-input"
				/>
				<span id="language-name-hint" class="text-xs text-muted-foreground">What editors see in lists and menus. Suggested from the code.</span>
				{#if showLabelProblem}<span id="language-name-problem" class="text-sm text-destructive" data-testid="language-name-problem">{labelProblem}</span>{/if}
			</div>
			<Dialog.Footer>
				<Button type="button" variant="outline" onclick={() => (adding = false)}>Cancel</Button>
				<Button type="submit" data-testid="add-language-submit">Add language</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>

<AlertDialog.Root bind:open={confirming}>
	<AlertDialog.Content onCloseAutoFocus={afterRemoveQuestion}>
		<AlertDialog.Header>
			<AlertDialog.Title>Remove {removing?.label}?</AlertDialog.Title>
			<AlertDialog.Description>
				Other languages stop showing it where they are missing{#if removing?.next}, and {removing.next} becomes the default{/if}. Nothing changes until you save; if entries
				are still written in it, saving says which.
			</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel>Keep it</AlertDialog.Cancel>
			<AlertDialog.Action variant="destructive" onclick={remove} data-testid="confirm-remove-language">Remove {removing?.label}</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>
