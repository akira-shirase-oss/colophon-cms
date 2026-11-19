<!--
  The entry's title as the page's heading, edited where it is read (D-184, after the audit found it
  four times on the page: the trail, an eyebrow, the heading and the Title field). One line: Enter
  moves on to the next field and a pasted line break becomes a space; it grows onto more lines
  when long. The count of characters shows from 80% of the limit; the field's help, a required
  title left empty and a refusal of it are said under it.
-->
<script lang="ts">
	import { onMount } from 'svelte';
	import type { FieldDefinition } from './api.ts';
	import { characters, type FormValue, fieldProblem } from './content.ts';
	import { patternMismatch, valueTaken } from './words.ts';

	let {
		field,
		value = $bindable(),
		locked = false,
		refused = null,
		onchange = () => {}
	}: {
		field: Extract<FieldDefinition, { type: 'text' }>;
		value: FormValue | undefined;
		locked?: boolean;
		/** The server's refusal of the last save, when it names this field. */
		refused?: { pointer: string | null; message: string; reason?: string | null } | null;
		onchange?: () => void;
	} = $props();

	const id = $props.id();
	let input = $state<HTMLTextAreaElement | null>(null);
	const text = $derived(typeof value === 'string' ? value : '');
	const count = $derived(characters(text));
	const counting = $derived(count >= Math.floor(field.max_chars * 0.8));
	const problem = $derived.by(() => {
		const here = refused?.pointer === `/${field.id}`;
		if (here && refused?.reason === 'pattern_mismatch') return patternMismatch(field.label, field.description);
		if (here && refused?.reason === 'value_taken') return valueTaken(field.label);
		return fieldProblem(field, value) || (here ? (refused?.message ?? '') : '');
	});
	const describedBy = $derived(
		[field.description ? `${id}-help` : '', counting ? `${id}-count` : '', problem ? `${id}-problem` : ''].filter((part) => part !== '').join(' ') || undefined
	);

	/** As tall as its lines, so a long title wraps like a heading instead of scrolling. */
	function fit() {
		if (input === null) return;
		input.style.height = 'auto';
		input.style.height = `${input.scrollHeight}px`;
	}
	$effect(() => {
		void text;
		fit();
	});
	onMount(() => {
		const observer = new ResizeObserver(fit);
		if (input !== null) observer.observe(input);
		return () => observer.disconnect();
	});

	function typed(event: Event & { currentTarget: HTMLTextAreaElement }) {
		const one = event.currentTarget.value.replace(/\s*\n\s*/g, ' ');
		if (one !== event.currentTarget.value) event.currentTarget.value = one;
		value = one;
		onchange();
	}

	/** Enter leaves the title for the first field below it, as Tab would. */
	function key(event: KeyboardEvent) {
		if (event.key !== 'Enter' || event.isComposing) return;
		event.preventDefault();
		const fields = [...document.querySelectorAll<HTMLElement>('#panel-content input, #panel-content textarea, #panel-content select, #panel-content [contenteditable="true"]')];
		fields.find((element) => !element.hasAttribute('disabled'))?.focus();
	}
</script>

<div class="mb-6 min-w-0" data-testid="entry-title">
	<!-- The heading's text is the title being edited: a heading takes its name from its content,
	     a text box's value included, so it reads as the title, and editing it is in place. -->
	<!-- svelte-ignore a11y_missing_content -->
	<h1 class="page-title">
		<textarea
			bind:this={input}
			id="{id}-input"
			rows="1"
			value={text}
			aria-label={field.label}
			aria-describedby={describedBy}
			aria-invalid={problem ? true : undefined}
			aria-required={field.required || undefined}
			readonly={locked}
			maxlength={field.max_chars}
			placeholder="Untitled"
			spellcheck="true"
			class="-mx-1 block w-[calc(100%+0.5rem)] resize-none overflow-hidden border-b-2 border-transparent bg-transparent px-1 pb-1 font-[inherit] tracking-[inherit] text-foreground outline-none placeholder:text-muted-foreground transition-colors hover:border-border focus-visible:border-ring read-only:hover:border-transparent aria-invalid:border-destructive"
			oninput={typed}
			onkeydown={key}
		></textarea>
	</h1>
	{#if field.description || locked || counting || problem}
		<div class="mt-2 grid gap-1 text-sm">
			{#if field.description}<p id="{id}-help" class="text-pretty text-muted-foreground" data-user-content>{field.description}</p>{/if}
			{#if locked}<p class="text-muted-foreground">Your role can’t change the {field.label.toLowerCase()}.</p>{/if}
			{#if counting}
				<p id="{id}-count" class="tabular {count > field.max_chars ? 'text-destructive' : 'text-muted-foreground'}" data-testid="title-count">
					{count.toLocaleString()} of {field.max_chars.toLocaleString()} characters
				</p>
			{/if}
			{#if problem}<p id="{id}-problem" class="text-destructive" data-testid="field-problem" data-field={field.id}>{problem}</p>{/if}
		</div>
	{/if}
</div>
