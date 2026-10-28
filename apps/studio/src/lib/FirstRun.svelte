<!--
  A project's first-run checklist (step 3.7, D-059 rule 4): write, review or publish, connect the
  website, invite a teammate, see a receipt. Each item is worked out from the project's real
  state, never ticked by hand, and links to the place that does it. Items a role can't do are
  left out (rule 5). Hidden, it stays hidden in this browser.
-->
<script lang="ts">
	import CircleCheckIcon from '@lucide/svelte/icons/circle-check';
	import CircleDashedIcon from '@lucide/svelte/icons/circle-dashed';
	import { resolve } from '$app/paths';
	import { createQuery } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui/ui/button';
	import { type ChangesetSummary, type ContentType, listInvitations, listMembers, may, type Project, type Release, type Summary } from './api.ts';
	import { hide, isHidden, oneOf, receiptSeen } from './firstRun.ts';
	import { destinationsQuery } from './queries.ts';

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

	// The kind of content to start with: a type the project added (Pages, for a documentation
	// site), else the built-in Articles.
	const main = $derived(types.find((type) => type.slug !== 'articles') ?? types[0]);
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
						action: latest ? 'Open it' : 'Write'
					}
				: {
						id: 'publish',
						title: 'Ask someone to review it',
						detail: 'Press Ask for review on it: a publisher approves it and puts it live.',
						done: sentForReview || releases.length > 0,
						href: entryHref,
						action: latest ? 'Open it' : 'Write'
					}
		].filter((item) => (item.id === 'write' ? writes : item.id === 'publish' ? selfPublish || asks : true));
		if (managesConnections) {
			list.push({
				id: 'connect',
				title: 'Connect your website',
				detail: 'So each release rebuilds your site, clears its CDN, or is checked on the live site.',
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
</script>

{#if !hidden}
	<section class="mb-10 rounded-2xl border border-border bg-card p-5 shadow-xs sm:p-6" aria-labelledby="first-run-heading" data-testid="first-run">
		<div class="mb-4 flex flex-wrap items-start justify-between gap-3">
			<div>
				<h2 id="first-run-heading" class="text-base font-semibold">{remaining === 0 ? "You're all set" : 'Get started'}</h2>
				<p class="text-sm text-muted-foreground" data-testid="first-run-count">
					{remaining === 0 ? 'Every step is done. You can hide this list.' : `${items.length - remaining} of ${items.length} done`}
				</p>
			</div>
			<Button
				variant="ghost"
				size="sm"
				onclick={() => {
					hide(identity, project);
					hidden = true;
				}}>Hide this list</Button
			>
		</div>
		<ol class="divide-y divide-dotted divide-border">
			{#each items as item (item.id)}
				<li class="flex items-center gap-3 py-3" data-testid="first-run-item" data-item={item.id} data-done={item.done}>
					{#if item.done}
						<CircleCheckIcon class="size-5 shrink-0 text-live" aria-hidden="true" />
					{:else}
						<CircleDashedIcon class="size-5 shrink-0 text-muted-foreground" aria-hidden="true" />
					{/if}
					<span class="min-w-0 flex-1">
						<span class="block text-sm font-medium" class:text-muted-foreground={item.done}>
							{item.title}<span class="sr-only">{item.done ? ' (done)' : ' (to do)'}</span>
						</span>
						<span class="block text-sm text-muted-foreground">{item.detail}</span>
					</span>
					{#if !item.done}
						<Button variant="outline" size="sm" href={item.href} aria-label="{item.action}: {item.title}">{item.action}</Button>
					{/if}
				</li>
			{/each}
		</ol>
	</section>
{/if}
