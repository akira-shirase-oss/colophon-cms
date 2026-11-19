<!--
  The entry's language and its translations, as one row (D-184, replacing D-087's Locale input
  and Versions card): "French — fr ▾ · Translate…". The menu lists the entry's translations, each
  opening its own editor, and changes this entry's language. Translate… picks a language by name,
  names this entry's own language first when it has none (a group needs each member named), and
  makes the translation in one step. Nothing is offered that the server would refuse: languages
  the group already has aren't listed, a typed tag is checked as the server checks it, and it
  waits for the draft to be saved, so the copy starts from what is on screen.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { goto } from '$app/navigation';
	import CheckIcon from '@lucide/svelte/icons/check';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import LanguagesIcon from '@lucide/svelte/icons/languages';
	import { useQueryClient } from '@tanstack/svelte-query';
	import { toast } from 'svelte-sonner';
	import { Button, buttonVariants } from '@colophon/ui/ui/button';
	import * as Dialog from '@colophon/ui/ui/dialog';
	import * as DropdownMenu from '@colophon/ui/ui/dropdown-menu';
	import { Input } from '@colophon/ui/ui/input';
	import * as NativeSelect from '@colophon/ui/ui/native-select';
	import { createLocalization, type Definition, type Draft } from './api.ts';
	import type { SaveStatus } from './saver.svelte.ts';
	import { plainError } from './errors.ts';
	import { browserLanguage, COMMON_LANGUAGES, isLanguageTag, languageLabel, languageName, sameTag, translationChoices } from './languages.ts';

	let {
		project,
		collection,
		entry,
		definition,
		latest,
		title,
		saved,
		translates,
		changes,
		setLocale
	}: {
		project: string;
		collection: string;
		entry: string;
		definition: Definition;
		/** The draft as last saved: its language and its group. */
		latest: Draft;
		/** The saved title, for the dialog's heading. */
		title: string;
		/** Whether the draft is saved: a translation copies the saved draft. */
		saved: boolean;
		/** Whether the role may make entries here (a translation is one). */
		translates: boolean;
		/** Whether the role may change this entry (its language is part of it). */
		changes: boolean;
		/** Saves this entry's language; answers how the save ended. */
		setLocale: (tag: string) => Promise<SaveStatus>;
	} = $props();

	const queryClient = useQueryClient();
	const OTHER = '*';
	const own = $derived(latest.locale ?? '');
	const grouped = $derived(latest.localizations.length > 0);
	const siblings = $derived(latest.localizations.filter((sibling) => sibling.id !== entry));
	/** The tags the group holds, this entry's included: a translation can't take one again. */
	const taken = $derived([own, ...latest.localizations.map((sibling) => sibling.locale ?? '')].filter((tag) => tag !== ''));
	const named = (tag: string) => (tag === '' ? 'Default language' : languageLabel(tag));

	// A unique field holds a value no other entry may share, and a translation copies it: the
	// server would refuse the copy, so Translate says why instead of trying.
	const uniqueHeld = $derived(definition.fields.filter((field) => field.unique === true && latest.content[field.id] !== undefined && latest.content[field.id] !== ''));
	const blocked = $derived(
		!translates
			? 'Your role can’t make entries here, so it can’t make a translation.'
			: !changes && own === ''
				? 'This entry needs a language first, and your role can’t change it.'
				: uniqueHeld.length > 0
					? `A translation would copy ${uniqueHeld.map((field) => field.label).join(', ')}, which no two entries may share.`
					: ''
	);

	// The dialog: translating, or changing this entry's own language.
	let mode = $state<'translate' | 'change' | null>(null);
	let source = $state('');
	let target = $state('');
	let typed = $state('');
	let busy = $state(false);
	let problem = $state('');

	function open(next: 'translate' | 'change') {
		problem = '';
		typed = '';
		source = own === '' ? browserLanguage() : own;
		if (next === 'translate') {
			const choices = translationChoices([...taken, source]);
			const wanted = choices.find((tag) => !sameTag(tag, source)) ?? '';
			target = wanted;
		} else {
			target = own;
		}
		mode = next;
	}

	/** The tag chosen, a typed one when "Another language" is chosen. */
	const pick = (choice: string) => (choice === OTHER ? typed.trim() : choice);
	const sourceTag = $derived(pick(source));
	const targetTag = $derived(pick(target));
	const translateChoices = $derived(translationChoices([...taken, ...(own === '' ? [] : [own])]));
	/** What stops the dialog's button, said beside it; '' when it may go. */
	const waiting = $derived.by(() => {
		if (mode === null) return '';
		if (!saved) return 'Saving your changes first…';
		if (mode === 'change') {
			if (target === OTHER && !isLanguageTag(targetTag)) return 'Write a language tag like pt-PT or es-419.';
			if (targetTag === '' && grouped) return 'An entry with translations keeps a language.';
			if (sameTag(targetTag, own) && targetTag !== '' ) return '';
			if (taken.some((tag) => sameTag(tag, targetTag) && !sameTag(tag, own))) return 'Another translation already has that language.';
			return '';
		}
		if (own === '') {
			if (source === '') return 'Choose this entry’s language.';
			if (source === OTHER && !isLanguageTag(sourceTag)) return 'Write this entry’s language as a tag like pt-PT.';
		}
		if (target === '') return 'Choose a language to translate into.';
		if (target === OTHER && !isLanguageTag(targetTag)) return 'Write a language tag like pt-PT or es-419.';
		if (sameTag(targetTag, own === '' ? sourceTag : own)) return 'Choose a language other than this entry’s.';
		if (taken.some((tag) => sameTag(tag, targetTag))) return 'There is already a translation in that language.';
		return '';
	});
	const unchanged = $derived(mode === 'change' && (targetTag === own || sameTag(targetTag, own)));

	async function translate(event: SubmitEvent) {
		event.preventDefault();
		if (waiting !== '' || busy) return;
		busy = true;
		problem = '';
		try {
			// A group needs each member named: an entry without a language is given one first.
			if (own === '') {
				const status = await setLocale(sourceTag);
				if (status !== 'saved') {
					problem = 'This entry’s language couldn’t be saved, so the translation wasn’t made. The save state above says why.';
					return;
				}
			}
			const made = await createLocalization(project, collection, entry, targetTag);
			toast.success(`Made the ${languageName(targetTag)} translation. You’re editing it now.`);
			void queryClient.invalidateQueries({ queryKey: ['draft', project, collection] });
			void queryClient.invalidateQueries({ queryKey: ['entries', project, collection] });
			mode = null;
			await goto(resolve('/projects/[project]/[collection]/[entry]', { project, collection, entry: made.draft.id }));
		} catch (failure) {
			problem = plainError(failure, 'make translations here');
		} finally {
			busy = false;
		}
	}

	async function change(event: SubmitEvent) {
		event.preventDefault();
		if (waiting !== '' || busy) return;
		if (unchanged) {
			mode = null;
			return;
		}
		busy = true;
		problem = '';
		try {
			const status = await setLocale(targetTag);
			if (status === 'saved') {
				toast.success(targetTag === '' ? 'This entry is in the default language now.' : `This entry is in ${languageName(targetTag)} now.`);
				mode = null;
			} else {
				problem = 'The language wasn’t saved. The save state above says why.';
			}
		} finally {
			busy = false;
		}
	}
</script>

<div class="grid gap-1.5" data-testid="entry-language">
	<p class="text-sm text-muted-foreground" id="language-label-{entry}">Language</p>
	<div class="flex min-w-0 items-center justify-between gap-2">
		<DropdownMenu.Root>
			<DropdownMenu.Trigger
				class="{buttonVariants({ variant: 'ghost', size: 'sm' })} -ml-3 max-w-full min-w-0 justify-start"
				aria-describedby="language-label-{entry}"
				data-testid="language-menu"
			>
				<LanguagesIcon class="text-muted-foreground" /><span class="truncate" data-testid="entry-locale">{named(own)}</span><ChevronDownIcon class="-mr-1 text-muted-foreground" />
			</DropdownMenu.Trigger>
			<DropdownMenu.Content align="start" class="w-64">
				<DropdownMenu.Group>
					<DropdownMenu.GroupHeading>{grouped ? 'Translations' : 'No translations yet'}</DropdownMenu.GroupHeading>
					{#if grouped}
						<DropdownMenu.Item class="font-medium" aria-current="page" data-testid="translation-this">
							<CheckIcon />{named(own)}<span class="ml-auto text-muted-foreground">This one</span>
						</DropdownMenu.Item>
						{#each siblings as sibling (sibling.id)}
							<DropdownMenu.Item
								class="pl-8"
								onSelect={() => goto(resolve('/projects/[project]/[collection]/[entry]', { project, collection, entry: sibling.id }))}
								data-testid="translation"
							>
								{named(sibling.locale ?? '')}
							</DropdownMenu.Item>
						{/each}
					{/if}
				</DropdownMenu.Group>
				{#if changes}
					<DropdownMenu.Separator />
					<DropdownMenu.Item onSelect={() => open('change')} data-testid="change-language">Change this entry’s language…</DropdownMenu.Item>
				{/if}
			</DropdownMenu.Content>
		</DropdownMenu.Root>
		<Button
			variant="link"
			size="sm"
			class="h-9 shrink-0 px-1.5 aria-disabled:cursor-not-allowed aria-disabled:no-underline aria-disabled:opacity-60"
			aria-disabled={blocked !== '' || undefined}
			aria-describedby={blocked === '' ? undefined : `translate-blocked-${entry}`}
			title={blocked || undefined}
			onclick={() => blocked === '' && open('translate')}
			data-testid="translate">Translate…</Button
		>
	</div>
	{#if blocked !== ''}<p id="translate-blocked-{entry}" class="sr-only">{blocked}</p>{/if}
</div>

<Dialog.Root open={mode !== null} onOpenChange={(next) => !next && !busy && (mode = null)}>
	<Dialog.Content class="sm:max-w-md" data-testid="language-dialog">
		<form class="grid gap-6" onsubmit={mode === 'change' ? change : translate}>
			<Dialog.Header>
				<Dialog.Title>{mode === 'change' ? 'This entry’s language' : `Translate “${title || 'Untitled'}”`}</Dialog.Title>
				<Dialog.Description>
					{#if mode === 'change'}
						The language it is written in. Your site asks for each language by its tag.
					{:else}
						A copy in another language that you edit and publish on its own. It starts as this entry’s saved text.
					{/if}
				</Dialog.Description>
			</Dialog.Header>

			<div class="grid gap-5">
				{#if mode === 'translate' && own === ''}
					<div class="grid gap-1.5">
						<label for="language-source-{entry}" class="text-sm font-medium">This entry is in</label>
						<p class="text-sm text-muted-foreground" id="language-source-help-{entry}">It has no language yet; translations need one to belong to it.</p>
						<NativeSelect.Root id="language-source-{entry}" class="w-full" bind:value={source} aria-describedby="language-source-help-{entry}" data-testid="language-source">
							<NativeSelect.Option value="" disabled>Choose a language</NativeSelect.Option>
							{#each COMMON_LANGUAGES as tag (tag)}<NativeSelect.Option value={tag}>{languageLabel(tag)}</NativeSelect.Option>{/each}
							<NativeSelect.Option value={OTHER}>Another language…</NativeSelect.Option>
						</NativeSelect.Root>
						{#if source === OTHER}
							<Input bind:value={typed} placeholder="A tag, like pt-PT" aria-label="This entry’s language tag" autocomplete="off" spellcheck="false" class="font-mono" />
						{/if}
					</div>
				{/if}

				<div class="grid gap-1.5">
					<label for="language-target-{entry}" class="text-sm font-medium">{mode === 'change' ? 'Language' : 'Translate into'}</label>
					<NativeSelect.Root id="language-target-{entry}" class="w-full" bind:value={target} data-testid="language-target">
						{#if mode === 'change'}
							<NativeSelect.Option value="" disabled={grouped}>Default language</NativeSelect.Option>
							{#if own !== '' && !COMMON_LANGUAGES.some((tag) => tag === own)}<NativeSelect.Option value={own}>{languageLabel(own)}</NativeSelect.Option>{/if}
							{#each COMMON_LANGUAGES as tag (tag)}
								<NativeSelect.Option value={tag} disabled={taken.some((held) => sameTag(held, tag) && !sameTag(held, own))}>{languageLabel(tag)}</NativeSelect.Option>
							{/each}
						{:else}
							<NativeSelect.Option value="" disabled>Choose a language</NativeSelect.Option>
							{#each translateChoices.filter((tag) => !(own === '' && sameTag(tag, sourceTag))) as tag (tag)}
								<NativeSelect.Option value={tag}>{languageLabel(tag)}</NativeSelect.Option>
							{/each}
						{/if}
						<NativeSelect.Option value={OTHER}>Another language…</NativeSelect.Option>
					</NativeSelect.Root>
					{#if target === OTHER}
						<Input bind:value={typed} placeholder="A tag, like pt-PT or es-419" aria-label="Language tag" autocomplete="off" spellcheck="false" class="font-mono" data-testid="language-tag" />
					{/if}
				</div>
				{#if problem !== ''}<p role="alert" class="text-sm text-destructive" data-testid="language-problem">{problem}</p>{/if}
			</div>

			<Dialog.Footer class="items-center">
				{#if waiting !== ''}<p class="mr-auto text-sm text-muted-foreground" data-testid="language-waiting">{waiting}</p>{/if}
				<Button type="button" variant="secondary" disabled={busy} onclick={() => (mode = null)}>Cancel</Button>
				<Button type="submit" disabled={waiting !== '' || busy} data-testid="language-go">
					{#if busy}{mode === 'change' ? 'Saving…' : 'Translating…'}{:else}{mode === 'change' ? 'Save' : 'Create translation'}{/if}
				</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>
