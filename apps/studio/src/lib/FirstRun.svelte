<!--
  A project's first-run checklist (step 3.7, D-059 rule 4): decide what you'll write (content types,
  D-147), write, review or publish, connect the website, invite a teammate, see a receipt. Each item is worked out from the project's real
  state, never ticked by hand, and links to the place that does it. Items a role can't do are
  left out (rule 5). Hidden, it stays hidden in this browser.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { createQuery } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui/ui/button';
	import { type ChangesetSummary, type ContentType, listInvitations, listMembers, may, type Project, type Release, type Summary } from './api.ts';
	import { hide, isHidden, mainCollection, oneOf, receiptSeen } from './firstRun.ts';
	import { destinationsQuery } from './queries.ts';
	import LightLine from './shell/LightLine.svelte';
	import PixelIcon, { type PixelName } from './shell/PixelIcon.svelte';

	let {
		project,
		identity,
		mine,
		types,
		entries,
		requests,
		releases
	}: {
		project: string;
		identity: string;
		mine: Project;
		types: ContentType[];
		/** Every entry the overview loaded, newest edit first. */
		entries: { type: ContentType; entry: Summary }[];
		requests: ChangesetSummary[];
		releases: Release[];
	} = $props();

	const managesMembers = $derived(may(mine, 'members.manage'));
	const managesConnections = $derived(may(mine, 'destinations.manage'));
	const members = createQuery(() => ({ queryKey: ['members', project], queryFn: () => listMembers(project) }));
	const invitations = createQuery(() => ({
		queryKey: ['invitations', project],
		queryFn: () => listInvitations(project),
		enabled: managesMembers
	}));
	const connections = createQuery(() => ({ ...destinationsQuery(project), enabled: managesConnections }));

	// Read together, never behind a short-circuit (svelte-query only updates what has been read).
	const counts = $derived({
		members: members.data?.length ?? 0,
		invitations: invitations.data?.length ?? 0,
		connections: connections.data?.length ?? 0
	});

	// The overview is drawn again for each project, so these are read once per project.
	// svelte-ignore state_referenced_locally
	let hidden = $state(isHidden(identity, project));
	// svelte-ignore state_referenced_locally
	const seen = receiptSeen(project);

	// The kind of content to start with: the first collection the person made (Products, or Pages
	// for a documentation site), else the built-in Articles; never a one-entry type.
	const main = $derived(mainCollection(types));
	// Deciding what to write is done once the project has its own kinds of content: a type beside
	// the built-in Articles, Articles changed (the blog's URL name), or anything written.
	const decided = $derived(types.some((type) => type.slug !== 'articles' || type.version > 1) || entries.length > 0);
	const designs = $derived(may(mine, 'schema.change'));
	const latest = $derived(entries[0]);
	const live = $derived(releases[0]);
	const selfPublish = $derived(
		mine.self_review === true && may(mine, 'content.publish', main?.slug) && may(mine, 'content.review', main?.slug)
	);
	const sentForReview = $derived(requests.some((request) => request.status.state !== 'open'));

	interface Item {
		id: string;
		title: string;
		detail: string;
		done: boolean;
		href: string;
		action: string;
	}

	const items = $derived.by((): Item[] => {
		const write = main
			? resolve('/projects/[project]/[collection]', { project, collection: main.slug }) + '?new=1'
			: resolve('/projects/[project]', { project });
		const entryHref = latest
			? resolve('/projects/[project]/[collection]/[entry]', { project, collection: latest.type.slug, entry: latest.entry.id })
			: write;
		const writes = main !== undefined && may(mine, 'content.create', main.slug);
		const asks = main !== undefined && may(mine, 'content.update', main.slug);
		const list: Item[] = [
			{
				id: 'types',
				title: 'Decide what you’ll write (Content types)',
				detail: 'The kinds of content your site has, like products or pages, and the fields each one has.',
				done: decided,
				href: resolve('/projects/[project]/settings/types', { project }),
				action: 'Choose'
			},
			{
				id: 'write',
				title: `Write your first ${main ? oneOf(main.definition.name) : 'entry'}`,
				detail: 'It saves as you type, and stays a draft until it is published.',
				done: entries.length > 0,
				href: write,
				action: 'Write'
			},
			selfPublish
				? {
						id: 'publish',
						title: 'Publish it',
						detail: 'Press Publish on it: it goes live as a numbered release.',
						done: releases.length > 0,
						href: entryHref,
						// Until something is written there is nothing to publish, so the button says what comes first
						action: latest ? 'Publish' : 'Write first'
					}
				: {
						id: 'publish',
						title: 'Ask someone to review it',
						detail: 'Press Ask for review on it: a publisher approves it and puts it live.',
						done: sentForReview || releases.length > 0,
						href: entryHref,
						action: latest ? 'Ask for review' : 'Write first'
					}
		].filter((item) => (item.id === 'types' ? designs : item.id === 'write' ? writes : item.id === 'publish' ? selfPublish || asks : true));
		if (managesConnections) {
			list.push({
				id: 'connect',
				title: 'Connect your website',
				detail: 'So each release rebuilds your site, clears its cache, or is checked on the live site.',
				done: counts.connections > 0,
				href: resolve('/projects/[project]/settings/connections', { project }),
				action: 'Connect'
			});
		}
		if (managesMembers) {
			list.push({
				id: 'invite',
				title: 'Invite a teammate',
				detail: 'They get a link that works once, with the role you choose.',
				done: counts.members > 1 || counts.invitations > 0,
				href: resolve('/projects/[project]/members', { project }) + '#invite',
				action: 'Invite'
			});
		}
		list.push({
			id: 'receipt',
			title: 'See your first receipt',
			detail: live ? 'What went live, and what reached your website.' : 'Each release has one, once something is live.',
			done: live !== undefined && seen,
			href: live
				? resolve('/projects/[project]/releases/[generation]', { project, generation: String(live.generation) })
				: resolve('/projects/[project]/releases', { project }),
			action: 'See it'
		});
		return list;
	});
	const remaining = $derived(items.filter((item) => !item.done).length);
	// The one next step: the screen's one primary action (D-182); the others are outlines.
	const next = $derived(items.find((item) => !item.done)?.id);
	const ICON: Record<string, PixelName> = { types: 'blocks', write: 'pen', publish: 'stamp', connect: 'globe', invite: 'people', receipt: 'receipt' };
</script>

{#if !hidden}
	<!-- Studio v2 (D-210 addendum): the website hero's inverted panel (white on dark, black on light), a pixel
	     icon for each step that wiggles on hover or tap, the one next step in the primary blue and the rest as
	     outlines in the panel's ink, and a first-run line that lights up once (shell/LightLine). -->
	<section class="first-run mb-10" aria-labelledby="first-run-heading" data-testid="first-run">
		<div class="mb-6 flex flex-wrap items-start justify-between gap-3">
			<div class="space-y-2">
				<h2 id="first-run-heading" class="first-run-title">{remaining === 0 ? "You're all set" : 'Get started'}</h2>
				<p class="first-run-muted text-sm" data-testid="first-run-count">
					{remaining === 0 ? 'Every step is done. You can hide this list.' : `${items.length - remaining} of ${items.length} done`}
				</p>
				{#if remaining > 0}
					<LightLine
						class="first-run-line"
						text="Decide what you’ll write, write it, publish it, and Colophon prints a receipt of what went live."
						rest="var(--hero-muted)"
						on="var(--hero-fg)"
					/>
				{/if}
			</div>
			<Button
				variant="ghost"
				size="sm"
				class="hero-ghost"
				onclick={() => {
					hide(identity, project);
					hidden = true;
				}}>Hide this list</Button
			>
		</div>
		<ol class="steps">
			{#each items as item (item.id)}
				<li class="step" data-testid="first-run-item" data-item={item.id} data-done={item.done}>
					<span class="flex items-center gap-3">
						<PixelIcon name={item.done ? 'check' : (ICON[item.id] ?? 'pen')} size="2rem" wiggle />
						<span class="text-sm font-medium" class:first-run-muted={item.done}>
							{item.title}<span class="sr-only">{item.done ? ' (done)' : ' (to do)'}</span>
						</span>
					</span>
					<span class="first-run-muted block flex-1 text-sm">{item.detail}</span>
					{#if !item.done}
						<Button
							variant={item.id === next ? 'default' : 'outline'}
							size="sm"
							class={item.id === next ? '' : 'hero-outline'}
							href={item.href}
							aria-label="{item.action}: {item.title}">{item.action}</Button
						>
					{/if}
				</li>
			{/each}
		</ol>
	</section>
{/if}

<style>
	.first-run {
		/* the pixel icons' outline takes the panel's ink, so it reads on it */
		--px-outline: var(--hero-fg);
		padding: var(--card-padding);
		border-radius: var(--radius-card);
		background: var(--hero-bg);
		color: var(--hero-fg);
	}
	.first-run-title {
		font-family: var(--font-title);
		font-weight: 400;
		font-size: 2.5rem;
		line-height: 1.1;
		letter-spacing: 0;
	}
	.first-run-muted {
		color: var(--hero-muted);
	}
	:global(.first-run-line) {
		max-width: 40rem;
		font-size: 1.125rem;
		line-height: 1.4;
		font-weight: 500;
	}
	.steps {
		display: grid;
		gap: 0.75rem;
	}
	@media (min-width: 48rem) {
		.steps {
			grid-template-columns: repeat(2, minmax(0, 1fr));
		}
	}
	@media (min-width: 80rem) {
		.steps {
			grid-template-columns: repeat(3, minmax(0, 1fr));
		}
	}
	.step {
		display: flex;
		flex-direction: column;
		gap: 0.75rem;
		padding: 1rem 1.125rem 1.125rem;
		border: 1px solid var(--hero-line);
		border-radius: var(--radius-control);
	}
	.step :global([data-slot='button']) {
		align-self: flex-start;
	}
	/* The outline and ghost buttons in the panel's ink: a 1px edge and the label, a 9% ink ground on hover. */
	.first-run :global(a.hero-outline[data-slot='button']),
	.first-run :global(button.hero-ghost[data-slot='button']) {
		border-color: var(--hero-fg);
		background: transparent;
		color: var(--hero-fg);
	}
	.first-run :global(button.hero-ghost[data-slot='button']) {
		border-color: transparent;
	}
	.first-run :global(a.hero-outline[data-slot='button']:hover),
	.first-run :global(button.hero-ghost[data-slot='button']:hover) {
		background: var(--hero-hover);
	}
</style>
