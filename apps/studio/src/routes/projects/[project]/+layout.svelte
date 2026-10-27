<!--
  A project's shell: the warm-grey sidebar (where things are, how many, what's waiting) around one
  white page, with breadcrumbs, the command palette (⌘K) and the appearance menu above it. The
  sidebar's foot always says what readers have now: the live release and its label.
-->
<script lang="ts">
	import ArrowUpRightIcon from '@lucide/svelte/icons/arrow-up-right';
	import ChevronsUpDownIcon from '@lucide/svelte/icons/chevrons-up-down';
	import FileTextIcon from '@lucide/svelte/icons/file-text';
	import LayersIcon from '@lucide/svelte/icons/layers';
	import LayoutDashboardIcon from '@lucide/svelte/icons/layout-dashboard';
	import PackageCheckIcon from '@lucide/svelte/icons/package-check';
	import SearchIcon from '@lucide/svelte/icons/search';
	import Settings2Icon from '@lucide/svelte/icons/settings-2';
	import UsersIcon from '@lucide/svelte/icons/users';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery } from '@tanstack/svelte-query';
	import * as Breadcrumb from '@colophon/ui/ui/breadcrumb';
	import { Button } from '@colophon/ui/ui/button';
	import * as DropdownMenu from '@colophon/ui/ui/dropdown-menu';
	import * as Sidebar from '@colophon/ui/ui/sidebar';
	import { deliveryUrl } from '#lib/api.ts';
	import { changesetsQuery, meQuery, projectQuery, releasesQuery, typesQuery } from '#lib/queries.ts';
	import AppearanceMenu from '#lib/shell/AppearanceMenu.svelte';
	import CollectionCount from '#lib/shell/CollectionCount.svelte';
	import CommandPalette from '#lib/shell/CommandPalette.svelte';
	import Mark from '#lib/shell/Mark.svelte';
	import ReleaseLabel from '#lib/shell/ReleaseLabel.svelte';
	import type { LayoutProps } from './$types';

	let { children }: LayoutProps = $props();

	const project = $derived(page.params.project ?? '');
	const route = $derived(page.route.id ?? '');
	const who = createQuery(meQuery);
	const mine = createQuery(() => projectQuery(project));
	const types = createQuery(() => typesQuery(project));
	const sets = createQuery(() => changesetsQuery(project));
	const releases = createQuery(() => releasesQuery(project));
	const awaiting = $derived((sets.data ?? []).filter((set) => set.status.state === 'awaiting_review').length);
	const live = $derived(releases.data?.releases.find((release) => release.generation === releases.data?.generation));
	let searching = $state(false);

	const SECTION: Record<string, string> = {
		'/projects/[project]/changesets': 'Changesets',
		'/projects/[project]/changesets/[changeset]': 'Changesets',
		'/projects/[project]/releases': 'Releases',
		'/projects/[project]/releases/[generation]': 'Releases',
		'/projects/[project]/members': 'Members',
		'/projects/[project]/settings': 'Settings',
		'/projects/[project]/settings/types/[type]': 'Settings',
		'/projects/[project]/settings/roles/[role]': 'Settings'
	};
	const collectionName = $derived(
		types.data?.find((type) => type.slug === page.params.collection)?.definition.name ?? page.params.collection ?? ''
	);
	const crumbs = $derived.by(() => {
		const trail: { label: string; href?: string }[] = [];
		if (route.includes('[collection]')) {
			trail.push({ label: 'Content' });
			trail.push({
				label: collectionName,
				href: route.endsWith('[entry]') ? resolve('/projects/[project]/[collection]', { project, collection: page.params.collection ?? '' }) : undefined
			});
			if (route.endsWith('[entry]')) trail.push({ label: 'Entry' });
		} else if (SECTION[route]) {
			const section = SECTION[route];
			const detail = route.endsWith(']') && !route.endsWith('[project]');
			const href =
				section === 'Changesets'
					? resolve('/projects/[project]/changesets', { project })
					: section === 'Settings'
						? resolve('/projects/[project]/settings', { project })
						: resolve('/projects/[project]/releases', { project });
			trail.push({ label: section, href: detail ? href : undefined });
			if (route.endsWith('[generation]')) trail.push({ label: `Release ${page.params.generation}` });
			if (route.endsWith('[changeset]')) trail.push({ label: 'Changeset' });
			if (route.endsWith('[role]')) trail.push({ label: `Role ${page.params.role ?? ''}` });
			if (route.endsWith('[type]')) {
				trail.push({ label: types.data?.find((type) => type.slug === page.params.type)?.definition.name ?? page.params.type ?? '' });
			}
		} else {
			trail.push({ label: 'Overview' });
		}
		return trail;
	});
</script>

<Sidebar.Provider>
	<Sidebar.Root variant="inset" collapsible="icon">
		<!-- One landmark for the whole sidebar, so screen-reader users can jump to it. -->
		<nav aria-label="Project" class="flex min-h-0 flex-1 flex-col">
		<Sidebar.Header>
			<Sidebar.Menu>
				<Sidebar.MenuItem>
					<DropdownMenu.Root>
						<DropdownMenu.Trigger>
							{#snippet child({ props })}
								<Sidebar.MenuButton size="lg" {...props} class="data-[state=open]:bg-sidebar-accent">
									<span class="flex size-8 shrink-0 items-center justify-center rounded-lg border border-sidebar-border bg-background shadow-xs">
										<Mark size={16} />
									</span>
									<span class="grid min-w-0 flex-1 text-left leading-tight">
										<span class="truncate font-medium" data-testid="project-name">{mine.data?.name ?? '…'}</span>
										<span class="truncate text-xs text-muted-foreground">{mine.data?.slug ?? ''}</span>
									</span>
									<ChevronsUpDownIcon class="ml-auto text-muted-foreground" aria-hidden="true" />
									<span class="sr-only">: switch project</span>
								</Sidebar.MenuButton>
							{/snippet}
						</DropdownMenu.Trigger>
						<DropdownMenu.Content class="w-64" align="start">
							<DropdownMenu.Label class="text-xs text-muted-foreground">Projects</DropdownMenu.Label>
							{#each (who.data?.projects ?? []).slice(0, 12) as other (other.id)}
								<DropdownMenu.Item>
									{#snippet child({ props })}
										<a {...props} href={resolve('/projects/[project]', { project: other.id })}>
											<span class="flex size-6 items-center justify-center rounded-md border border-border text-xs font-semibold">{other.name.slice(0, 1).toUpperCase()}</span>
											<span class="truncate">{other.name}</span>
											{#if other.id === project}<span class="ml-auto text-xs text-muted-foreground">current</span>{/if}
										</a>
									{/snippet}
								</DropdownMenu.Item>
							{/each}
							<DropdownMenu.Separator />
							<DropdownMenu.Item>
								{#snippet child({ props })}<a {...props} href={resolve('/')}>All projects</a>{/snippet}
							</DropdownMenu.Item>
						</DropdownMenu.Content>
					</DropdownMenu.Root>
				</Sidebar.MenuItem>
			</Sidebar.Menu>
		</Sidebar.Header>

		<Sidebar.Content>
			<Sidebar.Group>
				<Sidebar.Menu>
					<Sidebar.MenuItem>
						<Sidebar.MenuButton isActive={route === '/projects/[project]'} tooltipContent="Overview">
							{#snippet child({ props })}
								<a href={resolve('/projects/[project]', { project })} {...props}><LayoutDashboardIcon /><span>Overview</span></a>
							{/snippet}
						</Sidebar.MenuButton>
					</Sidebar.MenuItem>
					<Sidebar.MenuItem>
						<Sidebar.MenuButton onclick={() => (searching = true)} tooltipContent="Search">
							<SearchIcon /><span>Search</span>
							<kbd class="ml-auto rounded border border-sidebar-border bg-background px-1.5 font-sans text-[0.6875rem] text-muted-foreground">⌘K</kbd>
						</Sidebar.MenuButton>
					</Sidebar.MenuItem>
				</Sidebar.Menu>
			</Sidebar.Group>

			<Sidebar.Group>
				<Sidebar.GroupLabel>Content</Sidebar.GroupLabel>
				<Sidebar.Menu data-testid="nav-collections">
					{#each types.data ?? [] as type (type.slug)}
						<Sidebar.MenuItem>
							<Sidebar.MenuButton isActive={page.params.collection === type.slug} tooltipContent={type.definition.name}>
								{#snippet child({ props })}
									<a href={resolve('/projects/[project]/[collection]', { project, collection: type.slug })} {...props}>
										<FileTextIcon /><span>{type.definition.name}</span>
									</a>
								{/snippet}
							</Sidebar.MenuButton>
							<CollectionCount {project} collection={type.slug} />
						</Sidebar.MenuItem>
					{/each}
				</Sidebar.Menu>
			</Sidebar.Group>

			<Sidebar.Group>
				<Sidebar.GroupLabel>Publishing</Sidebar.GroupLabel>
				<Sidebar.Menu>
					<Sidebar.MenuItem>
						<Sidebar.MenuButton isActive={route.startsWith('/projects/[project]/changesets')} tooltipContent="Changesets">
							{#snippet child({ props })}
								<a href={resolve('/projects/[project]/changesets', { project })} data-testid="changesets-link" {...props}><LayersIcon /><span>Changesets</span></a>
							{/snippet}
						</Sidebar.MenuButton>
						{#if awaiting > 0}
							<Sidebar.MenuBadge class="tabular bg-pending-soft text-pending-ink" title="{awaiting} awaiting review">{awaiting}</Sidebar.MenuBadge>
						{/if}
					</Sidebar.MenuItem>
					<Sidebar.MenuItem>
						<Sidebar.MenuButton isActive={route.startsWith('/projects/[project]/releases')} tooltipContent="Releases">
							{#snippet child({ props })}
								<a href={resolve('/projects/[project]/releases', { project })} data-testid="releases-link" {...props}><PackageCheckIcon /><span>Releases</span></a>
							{/snippet}
						</Sidebar.MenuButton>
						{#if releases.data && releases.data.generation > 0}
							<Sidebar.MenuBadge class="tabular">No.&nbsp;{releases.data.generation}</Sidebar.MenuBadge>
						{/if}
					</Sidebar.MenuItem>
				</Sidebar.Menu>
			</Sidebar.Group>

			<Sidebar.Group>
				<Sidebar.GroupLabel>Project</Sidebar.GroupLabel>
				<Sidebar.Menu>
					<Sidebar.MenuItem>
						<Sidebar.MenuButton isActive={route.startsWith('/projects/[project]/members')} tooltipContent="Members">
							{#snippet child({ props })}
								<a href={resolve('/projects/[project]/members', { project })} data-testid="members-link" {...props}><UsersIcon /><span>Members</span></a>
							{/snippet}
						</Sidebar.MenuButton>
					</Sidebar.MenuItem>
					<Sidebar.MenuItem>
						<Sidebar.MenuButton isActive={route.startsWith('/projects/[project]/settings')} tooltipContent="Settings">
							{#snippet child({ props })}
								<a href={resolve('/projects/[project]/settings', { project })} data-testid="settings-link" {...props}><Settings2Icon /><span>Settings</span></a>
							{/snippet}
						</Sidebar.MenuButton>
					</Sidebar.MenuItem>
				</Sidebar.Menu>
			</Sidebar.Group>
		</Sidebar.Content>

		<Sidebar.Footer class="group-data-[collapsible=icon]:hidden">
			<div class="rounded-xl border border-sidebar-border bg-background p-3 shadow-xs" data-testid="live-card">
				{#if releases.data === undefined}
					<p class="text-xs text-muted-foreground">Checking what's live…</p>
				{:else if live === undefined}
					<p class="text-sm font-medium">Nothing live yet</p>
					<p class="mt-0.5 text-xs text-muted-foreground">Publish a changeset to make release No. 1.</p>
				{:else}
					<div class="flex items-baseline justify-between gap-2">
						<span class="text-xs text-muted-foreground">Live now</span>
						<a class="text-sm font-semibold tabular hover:underline" href={resolve('/projects/[project]/releases/[generation]', { project, generation: String(live.generation) })}>No. {live.generation}</a>
					</div>
					<div class="mt-2"><ReleaseLabel release={live} testid="live-label" /></div>
				{/if}
				<a class="mt-3 flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground" href={deliveryUrl(project, 'articles')} data-sveltekit-reload target="_blank" rel="noopener">
					Public API<ArrowUpRightIcon class="size-3" aria-hidden="true" />
				</a>
			</div>
		</Sidebar.Footer>
		<Sidebar.Rail />
		</nav>
	</Sidebar.Root>

	<Sidebar.Inset class="min-w-0 border border-sidebar-border md:peer-data-[variant=inset]:shadow-xs">
		<header class="sticky top-0 z-10 flex h-14 shrink-0 items-center gap-2 rounded-t-2xl border-b border-border bg-background/90 px-4 backdrop-blur">
			<Sidebar.Trigger class="-ml-1 mr-1" aria-label="Toggle sidebar" />
			<Breadcrumb.Root>
				<Breadcrumb.List>
					<Breadcrumb.Item class="hidden md:inline-flex">
						<Breadcrumb.Link href={resolve('/projects/[project]', { project })}>{mine.data?.name ?? 'Project'}</Breadcrumb.Link>
					</Breadcrumb.Item>
					{#each crumbs as crumb, index (index)}
						<Breadcrumb.Separator class="hidden md:inline-flex" />
						<Breadcrumb.Item>
							{#if crumb.href}
								<Breadcrumb.Link href={crumb.href}>{crumb.label}</Breadcrumb.Link>
							{:else}
								<Breadcrumb.Page>{crumb.label}</Breadcrumb.Page>
							{/if}
						</Breadcrumb.Item>
					{/each}
				</Breadcrumb.List>
			</Breadcrumb.Root>
			<div class="ml-auto flex items-center gap-1">
				<Button variant="outline" size="sm" class="hidden w-56 justify-start text-muted-foreground sm:flex" onclick={() => (searching = true)}>
					<SearchIcon />Search entries…<kbd class="ml-auto font-sans text-[0.6875rem]">⌘K</kbd>
				</Button>
				<AppearanceMenu />
			</div>
		</header>
		{#key page.url.pathname}
			<div class="page-enter mx-auto w-full max-w-6xl flex-1 px-5 py-8 sm:px-8 lg:px-10">
				{@render children()}
			</div>
		{/key}
	</Sidebar.Inset>
</Sidebar.Provider>

<CommandPalette {project} types={types.data ?? []} bind:open={searching} />
