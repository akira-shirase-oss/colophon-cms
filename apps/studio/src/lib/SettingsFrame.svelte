<!--
  Settings' frame (D-081), laid out like the content-type builder's (D-079): a left panel with one
  page each for General, Roles, Website connections, API tokens and Activity, beside the one open.
  A page shows in the panel only to roles that can use it, as the server asks; opened by address,
  each page says why it's empty instead. On a phone the panel is a row of links above the page.
-->
<script lang="ts">
	import GlobeIcon from '@lucide/svelte/icons/globe';
	import HistoryIcon from '@lucide/svelte/icons/history';
	import KeyRoundIcon from '@lucide/svelte/icons/key-round';
	import ShieldCheckIcon from '@lucide/svelte/icons/shield-check';
	import SlidersHorizontalIcon from '@lucide/svelte/icons/sliders-horizontal';
	import type { Snippet } from 'svelte';
	import { resolve } from '$app/paths';
	import { createQuery } from '@tanstack/svelte-query';
	import { may } from './api.ts';
	import { projectQuery } from './queries.ts';

	type Place = 'general' | 'roles' | 'connections' | 'tokens' | 'activity';

	let { project, current, children }: { project: string; current: Place; children: Snippet } = $props();

	const mine = createQuery(() => projectQuery(project));
	const groups = $derived([
		{
			label: 'Project',
			items: [
				{ place: 'general', label: 'General', href: resolve('/projects/[project]/settings', { project }), icon: SlidersHorizontalIcon, testid: 'settings-general-link', shown: true },
				// Every member may read the roles, so anyone in Settings sees them.
				{ place: 'roles', label: 'Roles', href: resolve('/projects/[project]/settings/roles', { project }), icon: ShieldCheckIcon, testid: 'roles-link', shown: true },
				{
					place: 'connections',
					label: 'Website connections',
					href: resolve('/projects/[project]/settings/connections', { project }),
					icon: GlobeIcon,
					testid: 'connections-link',
					shown: may(mine.data, 'destinations.manage')
				}
			]
		},
		{
			label: 'Access',
			items: [
				{ place: 'tokens', label: 'API tokens', href: resolve('/projects/[project]/settings/tokens', { project }), icon: KeyRoundIcon, testid: 'tokens-link', shown: may(mine.data, 'tokens.manage') },
				{ place: 'activity', label: 'Activity', href: resolve('/projects/[project]/settings/activity', { project }), icon: HistoryIcon, testid: 'activity-link', shown: may(mine.data, 'audit.read') }
			]
		}
	].map((group) => ({ ...group, items: group.items.filter((item) => item.shown || item.place === current) })).filter((group) => group.items.length > 0));
</script>

<div class="grid gap-6 lg:grid-cols-[13rem_minmax(0,1fr)] lg:gap-8">
	<nav class="min-w-0" aria-label="Settings" data-testid="settings-nav">
		<div class="grid gap-2 lg:sticky lg:top-20 lg:gap-5">
			{#each groups as group (group.label)}
				<div role="group" aria-labelledby="settings-group-{group.label}">
					<h2 id="settings-group-{group.label}" class="eyebrow mb-1.5 hidden px-2.5 lg:block">{group.label}</h2>
					<ul class="flex flex-wrap gap-1 lg:grid lg:gap-0.5">
						{#each group.items as item (item.place)}
							{@const active = item.place === current}
							<li>
								<a
									href={item.href}
									class="flex items-center gap-2 rounded-lg px-2.5 py-1.5 text-sm outline-none hover:bg-accent focus-visible:ring-4 focus-visible:ring-ring/25 {active
										? 'bg-card font-medium shadow-hairline'
										: 'text-muted-foreground hover:text-foreground'}"
									aria-current={active ? 'page' : undefined}
									data-testid={item.testid}><item.icon class="size-4 shrink-0" aria-hidden="true" /><span class="truncate">{item.label}</span></a
								>
							</li>
						{/each}
					</ul>
				</div>
			{/each}
		</div>
	</nav>
	<div class="min-w-0">{@render children()}</div>
</div>
