<!--
  The content-type builder's frame (its own place in the Studio's navigation, like Strapi's
  Content-Type Builder): a left panel listing the project's collections, single types and shared
  components, with New content type and New component, beside the one being built. Making one asks
  only for a name (its address name is made from it) and, for a content type, whether it holds
  many entries or just one; it starts with a Title field and opens, ready for fields.
-->
<script lang="ts">
	import ComponentIcon from '@lucide/svelte/icons/component';
	import FileTextIcon from '@lucide/svelte/icons/file-text';
	import FilesIcon from '@lucide/svelte/icons/files';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import type { Snippet } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { toast } from 'svelte-sonner';
	import { Button } from '@colophon/ui/ui/button';
	import * as Dialog from '@colophon/ui/ui/dialog';
	import { Input } from '@colophon/ui/ui/input';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { ApiError, createComponent, createContentType, may, newKey } from './api.ts';
	import { slugify } from './content.ts';
	import { componentsQuery, projectQuery, typesQuery } from './queries.ts';

	let {
		project,
		current = null,
		children
	}: {
		project: string;
		/** What is open: a content type's or a component's slug. */
		current?: { kind: 'type' | 'component'; slug: string } | null;
		children: Snippet;
	} = $props();

	const queryClient = useQueryClient();
	const mine = createQuery(() => projectQuery(project));
	const types = createQuery(() => typesQuery(project));
	const components = createQuery(() => componentsQuery(project));
	// Making one takes a role that may change content types everywhere, as the server asks.
	const makes = $derived(may(mine.data, 'schema.change'));
	const collections = $derived((types.data ?? []).filter((type) => type.definition.single !== true));
	const singles = $derived((types.data ?? []).filter((type) => type.definition.single === true));

	// A new content type or component: a name, the address name made from it, and its one field.
	let making = $state<'type' | 'component' | null>(null);
	let name = $state('');
	let slug = $state('');
	let slugTouched = $state(false);
	let single = $state(false);
	let error = $state('');
	let busy = $state(false);
	let key = newKey();
	const suggested = $derived(slugify(name).slice(0, 63).replace(/-+$/, ''));
	const effective = $derived(slugTouched ? slug : suggested);

	function start(kind: 'type' | 'component') {
		making = kind;
		name = '';
		slug = '';
		slugTouched = false;
		single = false;
		error = '';
		key = newKey();
	}

	async function make(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		error = '';
		try {
			if (making === 'type') {
				const created = await createContentType(
					project,
					effective,
					{
						name: name.trim(),
						title_field: 'title',
						fields: [{ id: 'title', label: 'Title', type: 'text', required: true, max_chars: 200, multiline: false }],
						...(single ? { single: true } : {})
					},
					key
				);
				await queryClient.invalidateQueries({ queryKey: ['types', project] });
				toast.success(`Created ${created.type.definition.name}. Add its fields.`);
				making = null;
				await goto(resolve('/projects/[project]/settings/types/[type]', { project, type: created.type.slug }));
			} else {
				const created = await createComponent(
					project,
					effective,
					{ name: name.trim(), fields: [{ id: 'title', label: 'Title', type: 'text', required: false, max_chars: 200, multiline: false }] },
					key
				);
				await queryClient.invalidateQueries({ queryKey: ['components', project] });
				toast.success(`Created ${created.definition.name}. Add its fields.`);
				making = null;
				await goto(resolve('/projects/[project]/settings/components/[component]', { project, component: created.slug }));
			}
		} catch (failure) {
			error = failure instanceof ApiError ? failure.message : String(failure);
		} finally {
			busy = false;
		}
	}
</script>

{#snippet item(href: string, label: string, active: boolean, Icon: typeof FileTextIcon, testid: string)}
	<li>
		<a
			{href}
			class="flex items-center gap-2 rounded-lg px-2.5 py-1.5 text-sm outline-none hover:bg-accent focus-visible:ring-4 focus-visible:ring-ring/25 {active ? 'bg-card font-medium shadow-hairline' : 'text-muted-foreground hover:text-foreground'}"
			aria-current={active ? 'page' : undefined}
			data-testid={testid}><Icon class="size-4 shrink-0" aria-hidden="true" /><span class="truncate">{label}</span></a
		>
	</li>
{/snippet}

<div class="grid gap-8 lg:grid-cols-[14rem_minmax(0,1fr)]">
	<aside class="min-w-0" aria-label="Content types and components" data-testid="builder-nav">
		<div class="grid gap-5 lg:sticky lg:top-20">
			{#if types.isPending}
				<Skeleton class="h-32 w-full rounded-xl" />
			{:else if types.isError}
				<p class="px-2.5 text-xs text-destructive">Couldn't load them. Try again on the page.</p>
			{:else}
				<section aria-labelledby="builder-collections">
					<h2 id="builder-collections" class="eyebrow mb-1.5 px-2.5">Collections</h2>
					<ul class="grid gap-0.5">
						{#each collections as type (type.slug)}
							{@render item(resolve('/projects/[project]/settings/types/[type]', { project, type: type.slug }), type.definition.name, current?.kind === 'type' && current.slug === type.slug, FilesIcon, `builder-type-${type.slug}`)}
						{:else}
							<li class="px-2.5 text-xs text-muted-foreground">None yet.</li>
						{/each}
					</ul>
				</section>
				{#if singles.length > 0}
					<section aria-labelledby="builder-singles">
						<h2 id="builder-singles" class="eyebrow mb-1.5 px-2.5">Single types</h2>
						<ul class="grid gap-0.5">
							{#each singles as type (type.slug)}
								{@render item(resolve('/projects/[project]/settings/types/[type]', { project, type: type.slug }), type.definition.name, current?.kind === 'type' && current.slug === type.slug, FileTextIcon, `builder-type-${type.slug}`)}
							{/each}
						</ul>
					</section>
				{/if}
				{#if makes}
					<Button variant="outline" size="sm" class="justify-start" onclick={() => start('type')} data-testid="new-type"><PlusIcon data-icon="inline-start" />New content type</Button>
				{/if}
				<section aria-labelledby="builder-components">
					<h2 id="builder-components" class="eyebrow mb-1.5 px-2.5">Components</h2>
					<ul class="grid gap-0.5">
						{#each components.data ?? [] as shared (shared.slug)}
							{@render item(resolve('/projects/[project]/settings/components/[component]', { project, component: shared.slug }), shared.definition.name, current?.kind === 'component' && current.slug === shared.slug, ComponentIcon, `builder-component-${shared.slug}`)}
						{:else}
							<li class="px-2.5 text-xs text-muted-foreground">None yet. A component is a group of fields several content types share, like SEO.</li>
						{/each}
					</ul>
				</section>
				{#if makes}
					<Button variant="outline" size="sm" class="justify-start" onclick={() => start('component')} data-testid="new-component"><PlusIcon data-icon="inline-start" />New component</Button>
				{/if}
			{/if}
		</div>
	</aside>
	<div class="min-w-0">{@render children()}</div>
</div>

<Dialog.Root open={making !== null} onOpenChange={(open) => !open && (making = null)}>
	<Dialog.Content class="sm:max-w-md">
		<form class="grid gap-5" onsubmit={make} aria-label={making === 'type' ? 'New content type' : 'New component'}>
			<Dialog.Header>
				<Dialog.Title>{making === 'type' ? 'New content type' : 'New component'}</Dialog.Title>
				<Dialog.Description>
					{making === 'type' ? 'A kind of content, like Pages, Authors or Recipes. It starts with a Title; you add the rest next.' : 'A group of fields content types share, like SEO or an address. It starts with a Title; you add the rest next.'}
				</Dialog.Description>
			</Dialog.Header>
			<label class="grid gap-1.5 text-sm">
				<span class="font-medium">Name</span>
				<!-- svelte-ignore a11y_autofocus -->
				<Input id={making === 'type' ? 'type-name' : 'component-name'} bind:value={name} required maxlength={100} autofocus />
			</label>
			<label class="grid gap-1.5 text-sm">
				<span class="font-medium">{making === 'type' ? 'Address name' : 'Code name'}</span>
				<Input
					id={making === 'type' ? 'type-slug' : 'component-slug'}
					class="font-mono"
					value={effective}
					oninput={(event) => {
						slugTouched = true;
						slug = event.currentTarget.value;
					}}
					required
					maxlength={63}
				/>
				<span class="text-xs text-muted-foreground">
					{making === 'type' ? 'Where its entries live in addresses and the API. Made from the name; it can’t change later.' : 'How content types and the API refer to it. Made from the name; it can’t change later.'}
				</span>
			</label>
			{#if making === 'type'}
				<fieldset class="grid gap-2 text-sm">
					<legend class="mb-1.5 font-medium">It holds</legend>
					<label class="flex items-start gap-2.5 rounded-xl border border-border bg-card px-3 py-2.5 has-checked:border-ring">
						<input type="radio" class="mt-0.5 size-4 accent-primary" name="holds" checked={!single} onchange={() => (single = false)} />
						<span><span class="font-medium">Many entries</span><span class="block text-xs text-muted-foreground">A list, like articles or products.</span></span>
					</label>
					<label class="flex items-start gap-2.5 rounded-xl border border-border bg-card px-3 py-2.5 has-checked:border-ring">
						<input type="radio" class="mt-0.5 size-4 accent-primary" name="holds" checked={single} onchange={() => (single = true)} />
						<span><span class="font-medium">Just one entry</span><span class="block text-xs text-muted-foreground">Like site settings or a home page.</span></span>
					</label>
				</fieldset>
			{/if}
			{#if error}<p role="alert" class="text-sm text-destructive">{error}</p>{/if}
			<Dialog.Footer>
				<Button type="button" variant="outline" onclick={() => (making = null)}>Cancel</Button>
				<Button type="submit" disabled={busy || name.trim() === '' || effective === ''} data-testid="create">{busy ? 'Creating…' : 'Create'}</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>
