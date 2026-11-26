<!--
  Import content: bringing entries in from another CMS or from files. The page starts from where
  the content is now (Strapi, Sanity, Markdown files, another Colophon project, a JSON file), says
  how to get it out of there, and then does the work in two steps the server already has (D-086,
  D-114): a check that writes nothing (`?dry_run=true`) and says what each entry would do, then
  the import of the entries the check accepted, in batches. Everything lands as a draft.

  The server decides what is valid. This page only reads the files, turns a Markdown file into
  the document its front matter and body describe, cuts a long list into batches, and reports
  what came back, including a run that stopped half way (lib/importing.ts).
-->
<script lang="ts">
	import ShapesIcon from '@lucide/svelte/icons/shapes';
	import { onDestroy } from 'svelte';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import * as Alert from '@colophon/ui/ui/alert';
	import { Button } from '@colophon/ui/ui/button';
	import { Input } from '@colophon/ui/ui/input';
	import * as NativeSelect from '@colophon/ui/ui/native-select';
	import { Progress } from '@colophon/ui/ui/progress';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { ApiError, applyImport, type ImportOutcome, may, mayAny, newKey, planImport } from '#lib/api.ts';
	import { describe } from '#lib/errors.ts';
	import {
		autoMap,
		BATCH_ITEMS,
		batches,
		type FrontMatter,
		importable,
		isArchive,
		kindOf,
		kindsOf,
		type Mapping,
		mapRecord,
		markdownEntry,
		markdownFields,
		MAX_FILE_BYTES,
		MAX_ITEMS,
		MAX_MARKDOWN_BYTES,
		namesIn,
		parseFrontMatter,
		placeOf,
		readItems,
		refusalWords,
		retrying,
		type Run,
		runWords,
		slugFieldOf,
		type SourceId,
		suggestedKind,
		summarize,
		titleOf,
		unanswered,
		wrongSource
	} from '#lib/importing.ts';
	import { projectQuery, typesQuery } from '#lib/queries.ts';
	import EmptyState from '#lib/shell/EmptyState.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';
	import TechnicalDetails from '#lib/shell/TechnicalDetails.svelte';
	import { count } from '#lib/words.ts';

	const ISSUES = 'https://github.com/akira-shirase-oss/colophon-cms/issues';

	const project = $derived(page.params.project ?? '');
	const queryClient = useQueryClient();
	const mine = createQuery(() => projectQuery(project));
	const types = createQuery(() => typesQuery(project));
	// A single type holds one entry; a file of entries has nowhere to go in it.
	const collections = $derived((types.data ?? []).filter((type) => type.definition.single !== true));
	// Importing creates entries, so it needs what creating one needs, in that collection.
	const writable = $derived(collections.filter((type) => may(mine.data, 'content.create', type.slug)));
	const builds = $derived(mayAny(mine.data, ['schema.change']));

	const SOURCES: { id: SourceId; name: string; line: string }[] = [
		{ id: 'strapi', name: 'Strapi', line: 'Entries saved from its REST API, or its export.' },
		{ id: 'sanity', name: 'Sanity', line: 'A dataset export, with one document on each line.' },
		{ id: 'markdown', name: 'Markdown files', line: 'A folder of .md files with front matter: Astro, Hugo, Jekyll, Eleventy, Next.js blogs.' },
		{ id: 'colophon', name: 'Colophon', line: 'A .jsonl export from another Colophon project.' },
		{ id: 'json', name: 'JSON file', line: 'A list of entries from anywhere else.' }
	];

	let source = $state<SourceId | null>(null);
	let picked = $state('');
	// The one content type there is needs no choosing.
	const collection = $derived(writable.some((one) => one.slug === picked) ? picked : writable.length === 1 ? (writable[0]?.slug ?? '') : '');
	const type = $derived(writable.find((one) => one.slug === collection));
	const definition = $derived(type?.definition);

	// ---- The files, as read

	/** One Markdown file, or one item of a data file. `id` stays the same while the files do, so an
	 *  entry that was imported is known after the mapping changes. */
	interface Piece {
		id: string;
		/** The file it came from, with its folder when one was chosen. */
		file: string;
		/** Its place in that file, from 1. */
		place: number;
		front?: Extract<FrontMatter, { ok: true }>;
		item?: unknown;
	}
	interface FileProblem {
		file: string;
		problem: string;
		technical?: string;
	}
	let pieces = $state.raw<Piece[]>([]);
	let fileProblems = $state.raw<FileProblem[]>([]);
	/** Files in a chosen folder that aren't Markdown. */
	let passedOver = $state(0);
	let filesRead = $state(0);
	let reading = $state(false);
	/** Why these files can't be used at all. */
	let unusable = $state('');
	/** Entries already imported from these files (a run that stopped part way). */
	let landed = $state.raw<Set<string>>(new Set());

	// ---- Choices made about them

	const bodyFields = $derived(definition === undefined ? [] : markdownFields(definition));
	let bodyChoice = $state('');
	const bodyField = $derived(bodyFields.length === 1 ? (bodyFields[0]?.id ?? '') : bodyFields.some((field) => field.id === bodyChoice) ? bodyChoice : '');
	const slugField = $derived(definition === undefined ? undefined : slugFieldOf(definition));

	const kinds = $derived(source === null ? [] : kindsOf(pieces.map((piece) => piece.item), source));
	let kindChoice = $state('');
	const kind = $derived(kinds.some((one) => one.kind === kindChoice) ? kindChoice : (suggestedKind(kinds, collection) ?? ''));
	/** The items in play: every one, or the chosen kind's from a mixed export. */
	const chosen = $derived(source === null || kinds.length === 0 ? pieces : pieces.filter((piece) => kindOf(piece.item, source as SourceId) === kind));

	// Names are matched to fields for Markdown front matter and for a plain JSON file. Strapi,
	// Sanity and Colophon files go as they are: the server knows their shapes.
	const maps = $derived(source === 'markdown' || source === 'json');
	const records = $derived(
		!maps ? [] : chosen.flatMap((piece) => (piece.front !== undefined ? [piece.front.data] : isRecord(piece.item) ? [piece.item] : []))
	);
	const names = $derived(namesIn(records));
	const targets = $derived((definition?.fields ?? []).filter((field) => !(source === 'markdown' && field.id === bodyField)));
	const matched = $derived(autoMap(names, targets));
	let overrides = $state<Mapping>({});
	const mapping = $derived<Mapping>(
		Object.fromEntries(
			names.map((name) => {
				const picked = overrides[name];
				const valid = picked === null || targets.some((field) => field.id === picked);
				return [name, picked !== undefined && valid ? picked : (matched[name] ?? null)];
			})
		)
	);
	const leftOut = $derived(names.filter((name) => mapping[name] === null));
	/** Required fields nothing here fills: the check will refuse every entry for them. */
	const unfilled = $derived(
		!maps || definition === undefined
			? []
			: targets.filter(
					(field) =>
						field.required &&
						field.default === undefined &&
						field.when === undefined &&
						!(source === 'markdown' && field.type === 'slug') &&
						!Object.values(mapping).includes(field.id)
				)
	);

	function isRecord(value: unknown): value is Record<string, unknown> {
		return typeof value === 'object' && value !== null && !Array.isArray(value);
	}

	/** One entry to send: what goes to the server, and how to name it to a person. */
	interface Prepared {
		id: string;
		value: unknown;
		/** Its title, or its file for Markdown. */
		label: string;
		/** Where it sits: "Entry 3 of posts.json". */
		where: string;
	}
	const several = $derived(new Set(pieces.map((piece) => piece.file)).size > 1);
	const prepared = $derived.by((): Prepared[] => {
		if (source === null || definition === undefined) return [];
		return chosen
			.filter((piece) => !landed.has(piece.id))
			.map((piece) => {
				if (piece.front !== undefined) {
					return { id: piece.id, value: bodyField === '' ? {} : markdownEntry(piece.file, piece.front, mapping, definition, bodyField), label: piece.file, where: '' };
				}
				const value = source === 'json' && isRecord(piece.item) ? mapRecord(piece.item, mapping, definition.fields) : piece.item;
				return {
					id: piece.id,
					value,
					// The title as it will be stored, where the mapping made one; else as the source nests it.
					label: titleOf(value, definition.title_field) || titleOf(piece.item, definition.title_field),
					where: several ? `Entry ${piece.place} of ${piece.file}` : `Entry ${piece.place}`
				};
			});
	});
	const noBodyField = $derived(source === 'markdown' && definition !== undefined && bodyFields.length === 0);
	const ready = $derived(
		source !== null && definition !== undefined && unusable === '' && prepared.length > 0 && !(source === 'markdown' && bodyField === '') && !(kinds.length > 0 && kind === '')
	);

	// ---- The check and the import

	type Phase = 'idle' | 'checking' | 'checked' | 'importing' | 'ended';
	let phase = $state<Phase>('idle');
	let progress = $state({ done: 0, of: 0 });
	/** What the check answered, in `checkedItems`' order. */
	let outcomes = $state.raw<ImportOutcome[]>([]);
	let checkedItems = $state.raw<Prepared[]>([]);
	let checkFailure = $state<unknown>(null);
	const summary = $derived(definition === undefined || phase === 'idle' || phase === 'checking' ? null : summarize(outcomes, definition));
	const willImport = $derived(importable(outcomes).length);

	/** The import under way or ended: its batches, the key each is sent with, and how far it got. */
	interface Job {
		batches: Prepared[][];
		keys: string[];
		run: Run;
		created: number;
		updated: number;
		failure: unknown;
		/** The entry a refusal names, and the field. */
		refused: { label: string; where: string; field: string | null; text: string } | null;
	}
	let job = $state.raw<Job | null>(null);
	const said = $derived(job === null ? null : runWords(job.run));
	const busy = $derived(reading || phase === 'checking' || phase === 'importing');

	let alive = true;
	onDestroy(() => (alive = false));
	const pause = (ms: number) => new Promise((done) => setTimeout(done, ms));
	const sanityKind = () => (source === 'sanity' && kind !== '' ? kind : undefined);

	/** Anything chosen differently makes the last check about something else. */
	function changed() {
		phase = 'idle';
		outcomes = [];
		checkedItems = [];
		checkFailure = null;
		job = null;
	}

	function forgetFiles() {
		pieces = [];
		fileProblems = [];
		passedOver = 0;
		filesRead = 0;
		unusable = '';
		landed = new Set();
		overrides = {};
		kindChoice = '';
		changed();
	}

	function chooseSource(next: SourceId) {
		source = next;
		forgetFiles();
	}

	function chooseType(event: Event) {
		picked = (event.currentTarget as HTMLSelectElement).value;
		// Another content type is another place: nothing from these files is in it yet.
		landed = new Set();
		overrides = {};
		bodyChoice = '';
		changed();
	}

	let picker = $state(0);
	/** Opening a file chooser starts over: both choosers are emptied first, so choosing the same
	 *  file again (after fixing it) is read again, and the page never shows files that one chooser
	 *  names while it works from the other's. */
	function reopen() {
		for (const id of ['import-file', 'import-folder']) {
			const input = document.getElementById(id);
			if (input instanceof HTMLInputElement) input.value = '';
		}
		forgetFiles();
	}

	async function choose(event: Event) {
		const input = event.currentTarget as HTMLInputElement;
		const files = [...(input.files ?? [])];
		forgetFiles();
		if (files.length === 0 || source === null) return;
		reading = true;
		try {
			if (source === 'markdown') await readMarkdown(files);
			else await readData(files, source);
		} finally {
			reading = false;
		}
	}

	async function readMarkdown(files: File[]) {
		const markdown = files.filter((file) => /\.(md|markdown|mdx)$/i.test(file.name));
		passedOver = files.length - markdown.length;
		if (markdown.length === 0) {
			unusable = 'None of these is a Markdown file. Choose files that end in .md, .markdown or .mdx.';
			return;
		}
		if (markdown.length > MAX_ITEMS) {
			unusable = `That is ${markdown.length.toLocaleString()} files. One import takes up to ${MAX_ITEMS.toLocaleString()}; choose fewer and import the rest after.`;
			return;
		}
		const read: Piece[] = [];
		const problems: FileProblem[] = [];
		const path = (file: File) => (file.webkitRelativePath === '' ? file.name : file.webkitRelativePath);
		for (const file of markdown.sort((a, b) => path(a).localeCompare(path(b)))) {
			if (file.size > MAX_MARKDOWN_BYTES) {
				problems.push({ file: path(file), problem: 'It is too large to be one entry (over 4 MB).' });
				continue;
			}
			const front = parseFrontMatter(await file.text());
			if (front.ok) read.push({ id: path(file), file: path(file), place: 1, front });
			else problems.push({ file: path(file), problem: front.problem, technical: front.technical });
		}
		filesRead = read.length;
		pieces = read;
		fileProblems = problems;
	}

	async function readData(files: File[], from: SourceId) {
		const read: Piece[] = [];
		const problems: FileProblem[] = [];
		for (const [index, file] of files.sort((a, b) => a.name.localeCompare(b.name)).entries()) {
			if (isArchive(new Uint8Array(await file.slice(0, 4).arrayBuffer()))) {
				problems.push({ file: file.name, problem: 'This is an archive, not the file of entries. Unpack it first, then choose the file inside that holds the entries.' });
				continue;
			}
			if (file.size > MAX_FILE_BYTES) {
				problems.push({ file: file.name, problem: 'It is too large to read here (over 32 MB). Split it into smaller files and import them one after another.' });
				continue;
			}
			const items = readItems(await file.text());
			if (!items.ok) {
				problems.push({ file: file.name, problem: items.problem });
				continue;
			}
			filesRead += 1;
			for (const [place, item] of items.items.entries()) read.push({ id: `${index}:${place}`, file: file.name, place: place + 1, item });
		}
		fileProblems = problems;
		const items = read.map((piece) => piece.item);
		const wrong = wrongSource(items, from);
		const loose = from === 'json' ? read.find((piece) => !isRecord(piece.item)) : undefined;
		if (wrong !== null) unusable = wrong;
		else if (loose !== undefined) unusable = `Entry ${loose.place} of ${loose.file} isn’t a set of named fields, like {"title": "My post"}.`;
		else if (read.length > MAX_ITEMS) {
			unusable = `That is ${read.length.toLocaleString()} entries. One import takes up to ${MAX_ITEMS.toLocaleString()}; split the file and import the parts one after another.`;
		}
		pieces = read;
	}

	async function check() {
		if (!ready || definition === undefined) return;
		const items = prepared;
		const groups = batches(items);
		changed();
		phase = 'checking';
		progress = { done: 0, of: groups.length };
		const answers: ImportOutcome[] = [];
		try {
			for (const group of groups) {
				const answer = await retrying(() => planImport(project, collection, group.map((one) => one.value), sanityKind()), pause);
				// One outcome for each entry sent, or the answer isn't about this batch.
				if (answer.length !== group.length) throw new Error(`the check answered ${answer.length} outcomes for ${group.length} entries`);
				answers.push(...answer);
				progress = { done: progress.done + 1, of: groups.length };
				if (!alive) return;
			}
			checkedItems = items;
			outcomes = answers;
			phase = 'checked';
		} catch (failure) {
			checkFailure = failure;
			phase = 'idle';
		}
	}

	function startImport() {
		if (phase !== 'checked') return;
		const sending = importable(outcomes).flatMap((index) => checkedItems[index] ?? []);
		const groups = batches(sending);
		job = {
			batches: groups,
			keys: groups.map(() => newKey()),
			run: { sizes: groups.map((group) => group.length), done: 0, imported: 0, stopped: null },
			created: 0,
			updated: 0,
			failure: null,
			refused: null
		};
		void send();
	}

	/** Sends the batches not yet answered, in order, each with its own key: sending one again is
	 *  safe. Stops at the first that fails, and says how far it got. */
	async function send() {
		if (job === null || definition === undefined) return;
		let at: Job = { ...job, run: { ...job.run, stopped: null }, failure: null, refused: null };
		job = at;
		phase = 'importing';
		progress = { done: at.run.done, of: at.batches.length };
		for (let index = at.run.done; index < at.batches.length; index += 1) {
			const group = at.batches[index] ?? [];
			try {
				const answer = await retrying(() => applyImport(project, collection, group.map((one) => one.value), at.keys[index] ?? newKey(), sanityKind()), pause);
				const made = (status: ImportOutcome['status']) => answer.outcomes.filter((outcome) => outcome.status === status).length;
				at = {
					...at,
					run: { ...at.run, done: index + 1, imported: (at.run.imported ?? 0) + answer.imported },
					created: at.created + made('created'),
					updated: at.updated + made('updated')
				};
				landed = new Set([...landed, ...group.map((one) => one.id)]);
			} catch (failure) {
				at = { ...at, run: { ...at.run, stopped: { batch: index, how: unanswered(failure) ? 'unanswered' : 'refused' } }, failure, refused: named(failure, group) };
			}
			job = at;
			progress = { done: at.run.done, of: at.batches.length };
			if (at.run.stopped !== null || !alive) break;
		}
		phase = 'ended';
		if (at.run.done > 0) void queryClient.invalidateQueries({ queryKey: ['entries', project, collection] });
	}

	/** The entry a refused batch's refusal points at, and why, when it points at one. */
	function named(failure: unknown, group: Prepared[]): Job['refused'] {
		if (!(failure instanceof ApiError) || failure.failure !== 'refused' || definition === undefined) return null;
		const place = placeOf(failure.details?.pointer ?? '');
		const entry = place === null ? undefined : group[place.index];
		if (place === null || entry === undefined) return null;
		const words = refusalWords({ code: failure.reason ?? '', pointer: place.rest, message: failure.message }, definition);
		return { label: entry.label, where: entry.where, ...words };
	}

	/** After a run that stopped: what was imported stays out, and the rest is checked again. */
	function checkTheRest() {
		job = null;
		void check();
	}

	function another() {
		picker += 1;
		forgetFiles();
	}

	function leaving(event: BeforeUnloadEvent) {
		if (phase === 'importing') event.preventDefault();
	}

	const SHOWN = 50;
	const listed = (words: string[]) => (words.length < 2 ? words.join('') : `${words.slice(0, -1).join(', ')} and ${words.at(-1)}`);
	const entryWord = (n: number) => count(n, 'entry', 'entries');
	const fieldLabel = (id: string | null) => definition?.fields.find((field) => field.id === id)?.label ?? '';
</script>

<!-- An entry named to a person: its title or its file, then where it sits when both are known. -->
{#snippet entryName(label: string, where: string)}<span class="font-medium break-all" data-user-content>{label || where}</span>{#if label !== '' && where !== ''}{' '}({where}){/if}{/snippet}

<!-- Why, with the field it is about unless the words already name it ("Title is required."). -->
{#snippet reason(field: string | null, text: string)}{#if field !== null && !text.includes(field)}in {field}, {text.slice(0, 1).toLowerCase()}{text.slice(1)}{:else}{text}{/if}{/snippet}

<svelte:window onbeforeunload={leaving} />
<svelte:head><title>Import content · Colophon Studio</title></svelte:head>

<PageHeader title="Import content">
	{#snippet description()}
		Bring entries in from another CMS or from files. They arrive as drafts, and nothing is published until you publish it.
	{/snippet}
</PageHeader>

{#if mine.isPending || types.isPending}
	<Skeleton class="h-64 w-full max-w-3xl rounded-xl" />
{:else if mine.isError}
	<Problem error={mine.error} what="this project" retry={() => mine.refetch()} />
{:else if types.isError}
	<Problem error={types.error} what="content types" retry={() => types.refetch()} />
{:else if collections.length === 0}
	<EmptyState icon={ShapesIcon} title="No content types yet" testid="import-no-types">
		Imported entries go into a content type, such as Posts or Recipes, and this project has none that holds a list of entries.
		{#if builds}Make one first, then come back.{:else}Ask one of the project's owners to make one.{/if}
		{#snippet action()}
			{#if builds}<Button href={resolve('/projects/[project]/settings/types', { project })}>Make a content type</Button>{/if}
		{/snippet}
	</EmptyState>
{:else if writable.length === 0}
	<p class="max-w-3xl rounded-lg border border-border bg-card px-4 py-3 text-sm text-muted-foreground" role="status" data-testid="import-not-allowed">
		Your role can't create entries in this project, so it can't import any. If you need to, ask one of the project's owners.
	</p>
{:else}
	<div class="max-w-3xl space-y-10">
		<section aria-labelledby="import-source">
			<fieldset disabled={busy}>
				<legend id="import-source" class="mb-3 text-lg font-medium">Where is your content now?</legend>
				<div class="grid gap-2 sm:grid-cols-2" data-testid="import-sources">
					{#each SOURCES as option (option.id)}
						<label class="flex items-start gap-2.5 rounded-xl border border-border bg-card px-3.5 py-3 text-sm has-checked:border-ring has-[:focus-visible]:ring-4 has-[:focus-visible]:ring-ring/15">
							<input
								type="radio"
								class="mt-0.5 size-4 accent-primary"
								name="import-source"
								value={option.id}
								checked={source === option.id}
								onchange={() => chooseSource(option.id)}
								data-testid="source-{option.id}"
							/>
							<span>
								<span class="font-medium">{option.name}</span>
								<span class="block text-muted-foreground">{option.line}</span>
							</span>
						</label>
					{/each}
				</div>
			</fieldset>
			<p class="mt-3 text-sm text-muted-foreground" data-testid="import-unsupported">
				WordPress, Contentful, Ghost and CSV files can't be imported yet. If you need one of them,
				<a class="text-link underline" href={ISSUES} target="_blank" rel="noopener noreferrer">say so in the project's issues</a>.
			</p>
		</section>

		{#if source !== null}
			<section aria-labelledby="import-how" data-testid="import-steps">
				<h2 id="import-how" class="mb-3 text-lg font-medium">
					{#if source === 'markdown'}Get your files ready{:else if source === 'json'}Get your file ready{:else}Get your content out of {SOURCES.find((one) => one.id === source)?.name}{/if}
				</h2>
				<ol class="list-decimal space-y-2 pl-5 text-sm marker:text-muted-foreground">
					{#if source === 'strapi'}
						<li>
							In this project, have a content type whose fields have the same names in code as the fields in Strapi, such as title and summary. An entry with a field the
							content type doesn't have can't be imported, and the check names that field.
						</li>
						<li>
							Get the entries out of Strapi as JSON. Either save what Strapi's REST API answers for that collection (the JSON with “data” and “meta” in it; one file for each
							page of results is fine), or use Strapi's own export and take the file of entries from it, the one with an entry on each line.
						</li>
						<li>Choose the file or files below, then check them.</li>
					{:else if source === 'sanity'}
						<li>
							In this project, have a content type whose fields have the same names in code as the fields of the documents in Sanity. A document with a field the content type
							doesn't have can't be imported, and the check names that field.
						</li>
						<li>
							Export your dataset with Sanity's own export (its command-line tool has <code class="font-mono">sanity dataset export</code>). If that gives you an archive,
							unpack it and find the file of documents, the one ending in .ndjson, with a document on each line.
						</li>
						<li>Choose that file below, pick which kind of document goes into the content type, then check it.</li>
					{:else if source === 'markdown'}
						<li>
							In this project, have a content type with a Markdown field for the text, and fields for what the front matter holds, such as a title, a date and tags.
						</li>
						<li>
							Choose your Markdown files below, or the folder they are in. Front matter is the block between two --- lines at the top of each file.
						</li>
						<li>Look over how its names were matched to the content type's fields, change any that are wrong, then check the files.</li>
					{:else if source === 'colophon'}
						<li>In the other project, open the collection and press Export. It downloads a file ending in .jsonl, with an entry on each line.</li>
						<li>In this project, have a content type with the same fields, named the same in code.</li>
						<li>Choose the file below, then check it.</li>
					{:else}
						<li>Save your entries as one JSON file: a list of entries, like [{'{'}"title": "One"{'}'}, {'{'}"title": "Two"{'}'}], or one entry on each line.</li>
						<li>Choose the file below.</li>
						<li>Look over how its names were matched to the content type's fields, change any that are wrong, then check the file.</li>
					{/if}
				</ol>
				<p class="mt-3 text-sm text-muted-foreground">
					{#if source === 'strapi'}
						Strapi's own dates, status and ids are set aside, not stored. Fields that hold pictures or link to other entries can't be brought over yet; if the file has them,
						the check names each one.
					{:else if source === 'sanity'}
						Sanity's own dates and ids are set aside, not stored. Fields Sanity keeps as objects, such as its block text, pictures and references, can't be converted yet; the
						check names each one.
					{:else if source === 'markdown'}
						Pictures in the folder aren't uploaded; add them in Media afterwards. Front matter between +++ lines isn't read.
					{:else if source === 'colophon'}
						The file doesn't say which entry is which, so importing it twice adds the entries twice. Fields that link to other entries or hold pictures point at the other
						project; the check names each entry they stop.
					{:else}
						A spreadsheet or a CSV file needs saving as JSON first.
					{/if}
				</p>
			</section>

			<section aria-labelledby="import-files" class="space-y-5">
				<h2 id="import-files" class="text-lg font-medium">Choose where it goes, and the {source === 'markdown' ? 'files' : 'file'}</h2>
				<div class="grid gap-1.5 text-sm">
					<label class="font-medium" for="import-type">Content type</label>
					<NativeSelect.Root id="import-type" class="w-full sm:w-80" value={collection} onchange={chooseType} disabled={busy} data-testid="import-type">
						<NativeSelect.Option value="" disabled>Choose a content type</NativeSelect.Option>
						{#each writable as option (option.slug)}
							<NativeSelect.Option value={option.slug}>{option.definition.name}</NativeSelect.Option>
						{/each}
					</NativeSelect.Root>
					<span class="text-muted-foreground">
						Every entry goes into this one.
						{#if builds}<a class="text-link underline" href={resolve('/projects/[project]/settings/types', { project })}>Make or change content types</a>{/if}
					</span>
				</div>

				{#if noBodyField && definition !== undefined}
					<Alert.Root variant="destructive" data-testid="import-no-markdown-field">
						<Alert.Title>{definition.name} has no Markdown field</Alert.Title>
						<Alert.Description>
							The text of a Markdown file needs a Markdown field to go into, and this content type has none.
							{#if builds}
								<a class="underline" href={resolve('/projects/[project]/settings/types/[type]', { project, type: collection })}>Add a Markdown field to it</a>, or choose
								another content type.
							{:else}
								Choose another content type, or ask one of the project's owners to add a Markdown field.
							{/if}
						</Alert.Description>
					</Alert.Root>
				{:else if definition !== undefined}
					{#key `${source}:${picker}`}
						<div class="grid gap-1.5 text-sm">
							<label class="font-medium" for="import-file">{source === 'markdown' ? 'Markdown files' : source === 'json' || source === 'colophon' ? 'File' : 'File or files'}</label>
							<Input
								id="import-file"
								type="file"
								multiple={source !== 'colophon' && source !== 'json'}
								accept={source === 'markdown' ? '.md,.markdown,.mdx,text/markdown' : '.json,.jsonl,.ndjson,.txt,application/json'}
								onclick={reopen}
								onchange={choose}
								disabled={busy}
								data-testid="import-file"
							/>
							{#if source === 'markdown'}
								<label class="mt-2 font-medium" for="import-folder">Or a whole folder</label>
								<Input id="import-folder" type="file" webkitdirectory onclick={reopen} onchange={choose} disabled={busy} data-testid="import-folder" />
								<span class="text-muted-foreground">Files that aren't Markdown are passed over.</span>
							{/if}
						</div>
					{/key}

					<div role="status" aria-live="polite" class="space-y-3 text-sm" data-testid="import-read">
						{#if reading}
							<p class="text-muted-foreground">Reading…</p>
						{:else if unusable !== ''}
							<Alert.Root variant="destructive" data-testid="import-unusable">
								<Alert.Title>This can't be imported as it is</Alert.Title>
								<Alert.Description>{unusable}</Alert.Description>
							</Alert.Root>
						{:else if pieces.length > 0}
							<p data-testid="import-found">
								{count(filesRead, 'file', 'files')} read: {entryWord(pieces.length)}.
								{#if passedOver > 0}{count(passedOver, 'file that isn’t Markdown was', 'files that aren’t Markdown were')} passed over.{/if}
								{#if landed.size > 0}{entryWord(landed.size)} from {source === 'markdown' ? 'these files' : 'here'} {landed.size === 1 ? 'is' : 'are'} already imported and left out.{/if}
							</p>
						{/if}
						{#if fileProblems.length > 0}
							<div class="rounded-xl border border-attention/25 bg-attention-soft px-4 py-3 text-attention-ink" data-testid="import-file-problems">
								<p class="font-medium">{count(fileProblems.length, 'file', 'files')} couldn't be read, and {fileProblems.length === 1 ? 'is' : 'are'} left out</p>
								<ul class="mt-1.5 space-y-1">
									{#each fileProblems.slice(0, SHOWN) as one (one.file)}
										<li><span class="font-medium break-all" data-user-content>{one.file}</span>: {one.problem}</li>
									{/each}
								</ul>
								{#if fileProblems.length > SHOWN}<p class="mt-1.5">And {fileProblems.length - SHOWN} more.</p>{/if}
								{#if fileProblems.some((one) => one.technical !== undefined)}
									<TechnicalDetails class="mt-2">
										{#each fileProblems.filter((one) => one.technical !== undefined).slice(0, SHOWN) as one (one.file)}
											<p class="break-words">{one.file}: {one.technical}</p>
										{/each}
									</TechnicalDetails>
								{/if}
							</div>
						{/if}
					</div>

					{#if kinds.length > 0 && unusable === ''}
						<div class="grid gap-1.5 text-sm">
							<label class="font-medium" for="import-kind">Which kind goes into {definition.name}?</label>
							<NativeSelect.Root
								id="import-kind"
								class="w-full sm:w-80"
								value={kind}
								onchange={(event) => {
									kindChoice = (event.currentTarget as HTMLSelectElement).value;
									changed();
								}}
								disabled={busy}
								data-testid="import-kind"
							>
								{#each kinds as one (one.kind)}
									<NativeSelect.Option value={one.kind}>{one.kind} ({one.count.toLocaleString()})</NativeSelect.Option>
								{/each}
							</NativeSelect.Root>
							<span class="text-muted-foreground">
								{#if kinds.length > 1}
									{source === 'markdown' ? 'These files hold' : 'What you chose holds'} {kinds.length} kinds. The {entryWord(pieces.length - chosen.length)} of other kinds
									{pieces.length - chosen.length === 1 ? 'is' : 'are'} left out; import each kind into its own content type.
								{:else}
									Every entry in the file is of this kind.
								{/if}
							</span>
						</div>
					{/if}

					{#if source === 'markdown' && bodyFields.length > 1}
						<div class="grid gap-1.5 text-sm">
							<label class="font-medium" for="import-body">Where the text of each file goes</label>
							<NativeSelect.Root id="import-body" class="w-full sm:w-80" bind:value={bodyChoice} onchange={changed} disabled={busy} data-testid="import-body">
								<NativeSelect.Option value="" disabled>Choose a Markdown field</NativeSelect.Option>
								{#each bodyFields as field (field.id)}
									<NativeSelect.Option value={field.id}>{field.label}</NativeSelect.Option>
								{/each}
							</NativeSelect.Root>
							<span class="text-muted-foreground">{definition.name} has {bodyFields.length} Markdown fields; the text under the front matter goes into one.</span>
						</div>
					{/if}

					{#if maps && names.length > 0 && unusable === '' && !(source === 'markdown' && bodyField === '')}
						<div class="space-y-2 text-sm" data-testid="import-mapping">
							<h3 class="font-medium">{source === 'markdown' ? 'Front matter' : 'Names in the file'}, and the field each goes into</h3>
							<ul class="overflow-hidden rounded-xl border border-border bg-card">
								{#each names as name, index (name)}
									<li class="rule-dotted flex flex-wrap items-center justify-between gap-x-4 gap-y-1.5 px-4 py-2.5 last:border-b-0">
										<label class="min-w-0 font-mono break-all" for="import-map-{index}" data-user-content>{name}</label>
										<NativeSelect.Root
											id="import-map-{index}"
											size="sm"
											class="w-full sm:w-64"
											value={mapping[name] ?? ''}
											onchange={(event) => {
												const picked = (event.currentTarget as HTMLSelectElement).value;
												overrides = { ...overrides, [name]: picked === '' ? null : picked };
												changed();
											}}
											disabled={busy}
											data-testid="import-map"
											data-name={name}
										>
											<NativeSelect.Option value="">Leave out</NativeSelect.Option>
											{#each targets as field (field.id)}
												<NativeSelect.Option value={field.id}>{field.label}</NativeSelect.Option>
											{/each}
										</NativeSelect.Root>
									</li>
								{/each}
							</ul>
							<p class="text-muted-foreground" data-testid="import-left-out">
								{#if leftOut.length === 0}
									Every name goes into a field.
								{:else}
									{count(leftOut.length, 'name matches', 'names match')} no field and {leftOut.length === 1 ? 'is' : 'are'} left out of the import:
									<span data-user-content>{leftOut.join(', ')}</span>.
								{/if}
							</p>
							{#if source === 'markdown'}
								<p class="text-muted-foreground">
									The text under the front matter goes into {fieldLabel(bodyField)}.
									{#if slugField !== undefined}
										{slugField.label} comes from the front matter when a name is matched to it and a file has it, and from the file's name otherwise.
									{/if}
									A full date and time given for a date field keeps its date.
								</p>
							{/if}
							{#if unfilled.length > 0}
								<p class="text-attention-ink" data-testid="import-unfilled">
									Nothing here fills {listed(unfilled.map((field) => field.label))}, which {unfilled.length === 1 ? 'is' : 'are'} required. The check will say so for every entry
									until a name is matched to {unfilled.length === 1 ? 'it' : 'them'}.
								</p>
							{/if}
						</div>
					{/if}
				{/if}
			</section>

			{#if definition !== undefined && !noBodyField}
				<section aria-labelledby="import-run" class="space-y-4">
					<h2 id="import-run" class="text-lg font-medium">Check, then import</h2>
					<p class="text-sm text-muted-foreground">
						The check changes nothing: it says what each entry would do. The import then creates the entries as drafts. Nothing is published, and your website doesn't
						change, until you publish them.
					</p>

					{#if phase === 'checking' || phase === 'importing'}
						<div class="space-y-2 text-sm" role="status" aria-live="polite" data-testid="import-progress">
							<p>
								{phase === 'checking' ? 'Checking' : 'Importing'}{progress.of > 1 ? `: batch ${Math.min(progress.done + 1, progress.of)} of ${progress.of}` : '…'}
							</p>
							<Progress value={progress.done} max={Math.max(progress.of, 1)} aria-label={phase === 'checking' ? 'Checking' : 'Importing'} />
							{#if phase === 'importing'}<p class="text-muted-foreground">Keep this page open until it finishes.</p>{/if}
						</div>
					{/if}

					{#if checkFailure !== null}
						{@const failed = describe(checkFailure, 'import into this content type')}
						<Alert.Root variant="destructive" data-testid="import-check-failed">
							<Alert.Title>The check didn't finish: {failed.title.toLowerCase()}</Alert.Title>
							<Alert.Description>
								<p>{failed.text} Nothing was changed.</p>
								<TechnicalDetails class="mt-2"><p class="break-words">{failed.technical}</p></TechnicalDetails>
							</Alert.Description>
						</Alert.Root>
					{/if}

					{#if summary !== null && job === null}
						<div class="space-y-3 rounded-xl border border-border bg-card p-card text-sm" role="status" data-testid="import-summary">
							<h3 class="font-medium">What the import will do</h3>
							<dl class="grid grid-cols-2 gap-x-6 gap-y-3 sm:grid-cols-4">
								<div><dt class="text-muted-foreground">Will be created</dt><dd class="figure text-2xl" data-testid="will-create">{summary.create.toLocaleString()}</dd></div>
								<div><dt class="text-muted-foreground">Will be updated</dt><dd class="figure text-2xl" data-testid="will-update">{summary.update.toLocaleString()}</dd></div>
								<div><dt class="text-muted-foreground">Skipped</dt><dd class="figure text-2xl" data-testid="will-skip">{summary.skipped.toLocaleString()}</dd></div>
								<div><dt class="text-muted-foreground">Can't be imported</dt><dd class="figure text-2xl" data-testid="will-refuse">{summary.invalid.toLocaleString()}</dd></div>
							</dl>
							{#if summary.update > 0}
								<p class="text-muted-foreground">An entry is updated when the file names one that is already here: its draft changes, and what readers get stays as it is.</p>
							{/if}
							{#if summary.setAside.length > 0}
								<p class="text-muted-foreground">Set aside, not stored: <span class="font-mono" data-user-content>{summary.setAside.join(', ')}</span>.</p>
							{/if}
							{#if summary.problems.length > 0}
								<div data-testid="import-problems">
									<h4 class="font-medium">Why {summary.invalid === 1 ? 'one' : `${summary.invalid.toLocaleString()}`} can't be imported</h4>
									<ul class="mt-1.5 space-y-1.5">
										{#each summary.problems.slice(0, SHOWN) as problem (problem.index)}
											{@const entry = checkedItems[problem.index]}
											<li>
												{@render entryName(entry?.label ?? '', entry?.where ?? '')}:
												{@render reason(problem.field, problem.text)}
											</li>
										{/each}
									</ul>
									{#if summary.problems.length > SHOWN}
										<p class="mt-1.5 text-muted-foreground">And {(summary.problems.length - SHOWN).toLocaleString()} more. Fix these and check again to see the rest.</p>
									{/if}
									<p class="mt-2 text-muted-foreground">
										{#if willImport > 0}{summary.invalid === 1 ? 'It is' : 'These are'} left out; the other {entryWord(willImport)} can be imported now.{:else}Fix the file or the content type, then check again.{/if}
										The check stops at the first thing wrong with an entry, so one may have more.
									</p>
									<TechnicalDetails class="mt-2">
										{#each summary.problems.slice(0, SHOWN) as problem (problem.index)}
											<p class="break-words">{problem.technical}</p>
										{/each}
									</TechnicalDetails>
								</div>
							{/if}
							{#if checkedItems.length > BATCH_ITEMS}
								<p class="text-muted-foreground">
									Entries go in batches of {BATCH_ITEMS}, each checked on its own, so the check can't see two entries in different batches that clash, such as the same URL
									name. The import would stop at the second and say so.
								</p>
							{/if}
						</div>
					{/if}

					{#if job !== null && said !== null && phase === 'ended'}
						{#if said.ok}
							<div class="space-y-2 rounded-xl border border-border bg-card p-card text-sm" role="status" data-testid="import-done">
								<h3 class="font-medium">{said.title}</h3>
								<p>
									{said.text}
									{#if job.updated > 0}{entryWord(job.created)} new, {job.updated.toLocaleString()} updated.{/if}
								</p>
								{#if prepared.length > 0}
									<p class="text-muted-foreground" data-testid="import-not-all">
										{entryWord(prepared.length)} from {source === 'markdown' ? 'these files' : 'this file'} {prepared.length === 1 ? 'was' : 'were'} not imported: the check found
										{prepared.length === 1 ? 'it' : 'them'} can't be, or left {prepared.length === 1 ? 'it' : 'them'} out.
									</p>
								{/if}
							</div>
						{:else}
							{@const failed = describe(job.failure, 'import into this content type')}
							<Alert.Root variant="destructive" data-testid="import-stopped">
								<Alert.Title>{said.title}</Alert.Title>
								<Alert.Description>
									<p>{said.text}</p>
									{#if job.refused !== null}
										<p class="mt-1.5">
											{@render entryName(job.refused.label, job.refused.where)}:
											{@render reason(job.refused.field, job.refused.text)}
										</p>
									{:else}
										<p class="mt-1.5">{failed.text}</p>
									{/if}
									<TechnicalDetails class="mt-2"><p class="break-words">{failed.technical}</p></TechnicalDetails>
								</Alert.Description>
							</Alert.Root>
						{/if}
					{/if}

					<div class="flex flex-wrap items-center gap-2">
						{#if phase === 'ended' && said?.ok}
							<Button href={resolve('/projects/[project]/[collection]', { project, collection })} data-testid="import-open">Open {definition.name}</Button>
							{#if prepared.length > 0}<Button variant="outline" onclick={checkTheRest} data-testid="import-check-rest">Check what's left</Button>{/if}
							<Button variant="outline" onclick={another}>Import something else</Button>
						{:else if phase === 'ended' && job !== null}
							{#if job.run.stopped?.how === 'refused'}
								<Button onclick={checkTheRest} data-testid="import-check-rest">Check what's left</Button>
								<Button variant="outline" onclick={() => void send()} data-testid="import-retry">Try again</Button>
							{:else}
								<Button onclick={() => void send()} data-testid="import-retry">Try again</Button>
							{/if}
							{#if job.run.done > 0}
								<Button variant="outline" href={resolve('/projects/[project]/[collection]', { project, collection })}>Open {definition.name}</Button>
							{/if}
						{:else if phase === 'checked' && willImport > 0}
							<Button onclick={startImport} data-testid="import-apply">Import {entryWord(willImport)}</Button>
							<Button variant="outline" onclick={() => void check()} data-testid="import-check">Check again</Button>
						{:else}
							<Button onclick={() => void check()} disabled={!ready || busy} data-testid="import-check">
								{phase === 'checking' ? 'Checking…' : phase === 'importing' ? 'Importing…' : phase === 'checked' ? 'Check again' : source === 'markdown' ? 'Check the files' : 'Check the file'}
							</Button>
							{#if !ready && !busy && pieces.length === 0 && unusable === ''}
								<span class="text-sm text-muted-foreground">Choose {source === 'markdown' ? 'the files' : 'a file'} first.</span>
							{/if}
						{/if}
					</div>
				</section>
			{/if}
		{/if}
	</div>
{/if}

<TechnicalDetails class="mt-10">
	<p>
		The check and the import are one API call each per batch: the import with <span class="font-mono">?dry_run=true</span> writes nothing and answers an outcome for every
		item; without it, a batch lands whole or not at all, under an Idempotency-Key. The API takes up to 512 items in one call; the Studio sends {BATCH_ITEMS}.
	</p>
	<p class="font-mono break-all">POST /api/v1/projects/{project}/environments/production/collections/{collection === '' ? '{collection}' : collection}/import</p>
</TechnicalDetails>
