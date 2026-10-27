<script lang="ts">
	import ChevronsUpDownIcon from '@lucide/svelte/icons/chevrons-up-down';
	import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
	import CircleCheckIcon from '@lucide/svelte/icons/circle-check';
	import FileTextIcon from '@lucide/svelte/icons/file-text';
	import InboxIcon from '@lucide/svelte/icons/inbox';
	import { getLocalTimeZone, today } from '@internationalized/date';
	import * as Accordion from '@colophon/ui/ui/accordion';
	import * as Alert from '@colophon/ui/ui/alert';
	import { AspectRatio } from '@colophon/ui/ui/aspect-ratio';
	import * as Avatar from '@colophon/ui/ui/avatar';
	import { Badge } from '@colophon/ui/ui/badge';
	import * as Breadcrumb from '@colophon/ui/ui/breadcrumb';
	import { Button } from '@colophon/ui/ui/button';
	import { Calendar } from '@colophon/ui/ui/calendar';
	import * as Card from '@colophon/ui/ui/card';
	import * as Carousel from '@colophon/ui/ui/carousel';
	import * as Collapsible from '@colophon/ui/ui/collapsible';
	import * as Empty from '@colophon/ui/ui/empty';
	import * as Item from '@colophon/ui/ui/item';
	import * as Pagination from '@colophon/ui/ui/pagination';
	import { Progress } from '@colophon/ui/ui/progress';
	import * as Resizable from '@colophon/ui/ui/resizable';
	import { ScrollArea } from '@colophon/ui/ui/scroll-area';
	import { Separator } from '@colophon/ui/ui/separator';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { Spinner } from '@colophon/ui/ui/spinner';
	import * as Table from '@colophon/ui/ui/table';
	import * as Tabs from '@colophon/ui/ui/tabs';
	import Specimen from './Specimen.svelte';

	const ROWS = [
		{ title: 'Hello, world', state: 'live', updated: '2 min ago', release: 42 },
		{ title: 'Why Colophon keeps every revision', state: 'changed', updated: '1 h ago', release: 41 },
		{ title: 'Notes on release generations', state: 'draft', updated: 'Yesterday', release: null },
		{ title: 'The outbox, explained', state: 'live', updated: '3 days ago', release: 38 }
	] as const;
	const STATE = { live: 'live', changed: 'pending', draft: 'draft' } as const;
	let date = $state(today(getLocalTimeZone()));
	const releases = Array.from({ length: 24 }, (_, index) => 42 - index);
</script>

<div class="grid gap-12">
	<Specimen label="Tabs">
		<Tabs.Root value="entries" class="w-full">
			<Tabs.List>
				<Tabs.Trigger value="entries">Entries</Tabs.Trigger>
				<Tabs.Trigger value="changesets">Changesets</Tabs.Trigger>
				<Tabs.Trigger value="releases">Releases</Tabs.Trigger>
			</Tabs.List>
			<Tabs.Content value="entries" class="pt-4">
				<div class="overflow-hidden rounded-3xl border border-border bg-card shadow-hairline">
					<Table.Root>
						<Table.Header>
							<Table.Row>
								<Table.Head class="pl-6">Title</Table.Head>
								<Table.Head>State</Table.Head>
								<Table.Head>Updated</Table.Head>
								<Table.Head class="pr-6 text-right">Release</Table.Head>
							</Table.Row>
						</Table.Header>
						<Table.Body>
							{#each ROWS as row (row.title)}
								<Table.Row>
									<Table.Cell class="pl-6 font-medium">{row.title}</Table.Cell>
									<Table.Cell>
										<Badge variant={STATE[row.state]}>{row.state[0]?.toUpperCase() + row.state.slice(1)}</Badge>
									</Table.Cell>
									<Table.Cell class="text-muted-foreground">{row.updated}</Table.Cell>
									<Table.Cell class="tabular pr-6 text-right">{row.release ?? '—'}</Table.Cell>
								</Table.Row>
							{/each}
						</Table.Body>
					</Table.Root>
				</div>
			</Tabs.Content>
			<Tabs.Content value="changesets" class="pt-4 text-sm text-muted-foreground">Changesets pin exact revisions for review.</Tabs.Content>
			<Tabs.Content value="releases" class="pt-4 text-sm text-muted-foreground">Releases only move forward.</Tabs.Content>
		</Tabs.Root>
	</Specimen>

	<div class="grid gap-6 md:grid-cols-3">
		<Card.Root>
			<Card.Header>
				<Card.Description>Drafts</Card.Description>
				<Card.Title class="figure text-4xl font-normal">128</Card.Title>
			</Card.Header>
			<Card.Content class="text-sm text-muted-foreground">12 changed since the last release.</Card.Content>
		</Card.Root>
		<Card.Root>
			<Card.Header>
				<Card.Description>Live release</Card.Description>
				<Card.Title class="figure text-4xl font-normal">42</Card.Title>
				<Card.Action><Badge variant="live"><span class="size-1.5 rounded-full bg-live"></span>Verified</Badge></Card.Action>
			</Card.Header>
			<Card.Content class="text-sm text-muted-foreground">At 1 location, 4 seconds after commit.</Card.Content>
		</Card.Root>
		<Card.Root>
			<Card.Header>
				<Card.Title>Awaiting review</Card.Title>
				<Card.Description>Two changesets need a second pair of eyes.</Card.Description>
			</Card.Header>
			<Card.Footer><Button size="sm" variant="outline">Review</Button></Card.Footer>
		</Card.Root>
	</div>

	<div class="grid gap-10 lg:grid-cols-2">
		<Specimen label="Items">
			<Item.Group class="gap-3">
				<Item.Root variant="outline">
					<Item.Media variant="icon"><FileTextIcon /></Item.Media>
					<Item.Content>
						<Item.Title>Hello, world</Item.Title>
						<Item.Description>Changed: title and 2 paragraphs since release 41.</Item.Description>
					</Item.Content>
					<Item.Actions><Button size="sm" variant="outline">Compare</Button></Item.Actions>
				</Item.Root>
				<Item.Root variant="outline">
					<Item.Media>
						<Avatar.Group>
							<Avatar.Root><Avatar.Fallback>AL</Avatar.Fallback></Avatar.Root>
							<Avatar.Root><Avatar.Fallback>GH</Avatar.Fallback></Avatar.Root>
						</Avatar.Group>
					</Item.Media>
					<Item.Content>
						<Item.Title>Two reviewers approved</Item.Title>
						<Item.Description>The approval is bound to hash sha256:9f2c…e41a.</Item.Description>
					</Item.Content>
				</Item.Root>
			</Item.Group>
		</Specimen>

		<Specimen label="Empty state">
			<Empty.Root class="rounded-3xl border border-dashed border-border">
				<Empty.Header>
					<Empty.Media variant="icon"><InboxIcon /></Empty.Media>
					<Empty.Title>No changesets yet</Empty.Title>
					<Empty.Description>Pick entries to publish together; a reviewer approves the exact change.</Empty.Description>
				</Empty.Header>
				<Empty.Content><Button>New changeset</Button></Empty.Content>
			</Empty.Root>
		</Specimen>

		<Specimen label="Alerts">
			<div class="grid gap-3">
				<Alert.Root>
					<CircleCheckIcon />
					<Alert.Title>Verified at 1 location</Alert.Title>
					<Alert.Description>The probe read generation 42 at https://example.com.</Alert.Description>
				</Alert.Root>
				<Alert.Root variant="destructive">
					<CircleAlertIcon />
					<Alert.Title>Needs attention</Alert.Title>
					<Alert.Description>The CDN purge was accepted but not confirmed; readers may still get release 41.</Alert.Description>
				</Alert.Root>
			</div>
		</Specimen>

		<Specimen label="Progress, loading and skeletons">
			<div class="grid gap-5">
				<div class="grid gap-2">
					<p class="flex justify-between text-sm"><span>Delivering release 42</span><span class="tabular text-muted-foreground">2 of 3</span></p>
					<Progress value={66} />
				</div>
				<p class="flex items-center gap-2 text-sm text-muted-foreground"><Spinner />Waiting for the probe…</p>
				<div class="flex items-center gap-3">
					<Skeleton class="size-10 rounded-full" />
					<div class="grid flex-1 gap-2"><Skeleton class="h-3.5 w-2/3" /><Skeleton class="h-3.5 w-1/3" /></div>
				</div>
			</div>
		</Specimen>
	</div>

	<div class="grid gap-10 lg:grid-cols-3">
		<Specimen label="Accordion">
			<Accordion.Root type="single">
				<Accordion.Item value="a">
					<Accordion.Trigger>What is a generation?</Accordion.Trigger>
					<Accordion.Content>A number every release moves forward. Rollback is a new generation, never an older one.</Accordion.Content>
				</Accordion.Item>
				<Accordion.Item value="b">
					<Accordion.Trigger>What does verified mean?</Accordion.Trigger>
					<Accordion.Content>A probe read the release's marker at a location you configured, and nowhere else is claimed.</Accordion.Content>
				</Accordion.Item>
			</Accordion.Root>
		</Specimen>

		<Specimen label="Collapsible">
			<Collapsible.Root class="rounded-2xl border border-border bg-card p-4 shadow-hairline">
				<div class="flex items-center justify-between">
					<p class="text-sm font-medium">3 destinations</p>
					<Collapsible.Trigger class="rounded-full p-1.5 hover:bg-accent" aria-label="Show destinations"><ChevronsUpDownIcon class="size-4" /></Collapsible.Trigger>
				</div>
				<Collapsible.Content class="mt-3 grid gap-2 font-mono text-xs text-muted-foreground">
					<p>webhook · site builder</p>
					<p>cdn · purge by tag</p>
					<p>probe · eu-west</p>
				</Collapsible.Content>
			</Collapsible.Root>
		</Specimen>

		<Specimen label="Breadcrumb and pagination">
			<div class="grid gap-6">
				<Breadcrumb.Root>
					<Breadcrumb.List>
						<Breadcrumb.Item><Breadcrumb.Link href="#data">my-site</Breadcrumb.Link></Breadcrumb.Item>
						<Breadcrumb.Separator />
						<Breadcrumb.Item><Breadcrumb.Link href="#data">Articles</Breadcrumb.Link></Breadcrumb.Item>
						<Breadcrumb.Separator />
						<Breadcrumb.Item><Breadcrumb.Page>Hello, world</Breadcrumb.Page></Breadcrumb.Item>
					</Breadcrumb.List>
				</Breadcrumb.Root>
				<Pagination.Root count={120} perPage={20}>
					{#snippet children({ pages, currentPage })}
						<Pagination.Content>
							<Pagination.Item><Pagination.Previous /></Pagination.Item>
							{#each pages as page (page.key)}
								{#if page.type === 'ellipsis'}
									<Pagination.Item><Pagination.Ellipsis /></Pagination.Item>
								{:else}
									<Pagination.Item><Pagination.Link {page} isActive={currentPage === page.value}>{page.value}</Pagination.Link></Pagination.Item>
								{/if}
							{/each}
							<Pagination.Item><Pagination.Next /></Pagination.Item>
						</Pagination.Content>
					{/snippet}
				</Pagination.Root>
			</div>
		</Specimen>
	</div>

	<div class="grid gap-10 lg:grid-cols-3">
		<Specimen label="Calendar: schedule a release">
			<Calendar type="single" bind:value={date} class="w-fit rounded-3xl border border-border bg-card shadow-hairline" />
		</Specimen>

		<Specimen label="Carousel">
			<Carousel.Root class="mx-12 max-w-xs">
				<Carousel.Content>
					{#each [40, 41, 42] as release (release)}
						<Carousel.Item>
							<div class="glow grid aspect-[4/3] place-items-center rounded-3xl">
								<span class="figure text-5xl">{release}</span>
							</div>
						</Carousel.Item>
					{/each}
				</Carousel.Content>
				<Carousel.Previous />
				<Carousel.Next />
			</Carousel.Root>
		</Specimen>

		<Specimen label="Scroll area and separators">
			<ScrollArea class="h-64 rounded-3xl border border-border bg-card shadow-hairline">
				<div class="p-4">
					<p class="mb-3 text-sm font-medium">Releases</p>
					{#each releases as release, index (release)}
						{#if index > 0}<Separator class="my-2" />{/if}
						<p class="flex justify-between text-sm"><span>Release {release}</span><span class="tabular text-muted-foreground">gen {release}</span></p>
					{/each}
				</div>
			</ScrollArea>
		</Specimen>
	</div>

	<div class="grid gap-10 lg:grid-cols-2">
		<Specimen label="Resizable: the editor and its preview">
			<Resizable.PaneGroup direction="horizontal" class="min-h-48 rounded-3xl border border-border bg-card shadow-hairline">
				<Resizable.Pane defaultSize={55}><div class="grid h-full place-items-center p-6 text-sm">Editor</div></Resizable.Pane>
				<Resizable.Handle withHandle />
				<Resizable.Pane defaultSize={45}><div class="grid h-full place-items-center p-6 text-sm text-muted-foreground">Preview</div></Resizable.Pane>
			</Resizable.PaneGroup>
		</Specimen>

		<Specimen label="Aspect ratio">
			<div class="max-w-sm">
				<AspectRatio ratio={16 / 9} class="glow overflow-hidden rounded-3xl">
					<div class="grid h-full place-items-center text-sm font-medium">16 : 9 cover</div>
				</AspectRatio>
			</div>
		</Specimen>
	</div>
</div>
