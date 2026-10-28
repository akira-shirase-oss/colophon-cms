<script lang="ts">
	import FileTextIcon from '@lucide/svelte/icons/file-text';
	import RocketIcon from '@lucide/svelte/icons/rocket';
	import SettingsIcon from '@lucide/svelte/icons/settings';
	import { toast } from 'svelte-sonner';
	import * as AlertDialog from '@colophon/ui/ui/alert-dialog';
	import * as Avatar from '@colophon/ui/ui/avatar';
	import { Button, buttonVariants } from '@colophon/ui/ui/button';
	import * as Command from '@colophon/ui/ui/command';
	import * as ContextMenu from '@colophon/ui/ui/context-menu';
	import * as Dialog from '@colophon/ui/ui/dialog';
	import * as Drawer from '@colophon/ui/ui/drawer';
	import * as DropdownMenu from '@colophon/ui/ui/dropdown-menu';
	import * as HoverCard from '@colophon/ui/ui/hover-card';
	import { Input } from '@colophon/ui/ui/input';
	import { Label } from '@colophon/ui/ui/label';
	import * as Menubar from '@colophon/ui/ui/menubar';
	import * as NavigationMenu from '@colophon/ui/ui/navigation-menu';
	import * as Popover from '@colophon/ui/ui/popover';
	import * as Sheet from '@colophon/ui/ui/sheet';
	import Specimen from './Specimen.svelte';
</script>

<div class="grid gap-12">
	<div class="grid gap-10 sm:grid-cols-2 lg:grid-cols-4">
		<Specimen label="Dialog">
			<Dialog.Root>
				<Dialog.Trigger class={buttonVariants({ variant: 'outline' })}>New entry</Dialog.Trigger>
				<Dialog.Content class="sm:max-w-md">
					<Dialog.Header>
						<Dialog.Title>New article</Dialog.Title>
						<Dialog.Description>A draft only you and your reviewers can see until it is published.</Dialog.Description>
					</Dialog.Header>
					<div class="grid gap-2"><Label for="d-title">Title</Label><Input id="d-title" placeholder="Hello, world" /></div>
					<Dialog.Footer>
						<Dialog.Close class={buttonVariants({ variant: 'outline' })}>Cancel</Dialog.Close>
						<Button>Create draft</Button>
					</Dialog.Footer>
				</Dialog.Content>
			</Dialog.Root>
		</Specimen>

		<Specimen label="Alert dialog">
			<AlertDialog.Root>
				<AlertDialog.Trigger class={buttonVariants({ variant: 'outline' })}>Unpublish</AlertDialog.Trigger>
				<AlertDialog.Content>
					<AlertDialog.Header>
						<AlertDialog.Title>Take this article down?</AlertDialog.Title>
						<AlertDialog.Description>
							Readers stop getting it at the next release, through every old URL too. Caches that still hold it are listed until they are purged.
						</AlertDialog.Description>
					</AlertDialog.Header>
					<AlertDialog.Footer>
						<AlertDialog.Cancel>Keep it</AlertDialog.Cancel>
						<AlertDialog.Action variant="destructive">Unpublish</AlertDialog.Action>
					</AlertDialog.Footer>
				</AlertDialog.Content>
			</AlertDialog.Root>
		</Specimen>

		<Specimen label="Sheet">
			<Sheet.Root>
				<Sheet.Trigger class={buttonVariants({ variant: 'outline' })}>Open history</Sheet.Trigger>
				<Sheet.Content>
					<Sheet.Header>
						<Sheet.Title>History</Sheet.Title>
						<Sheet.Description>Every revision is kept; restoring one makes a new revision.</Sheet.Description>
					</Sheet.Header>
				</Sheet.Content>
			</Sheet.Root>
		</Specimen>

		<Specimen label="Drawer">
			<Drawer.Root>
				<Drawer.Trigger class={buttonVariants({ variant: 'outline' })}>Receipt</Drawer.Trigger>
				<Drawer.Content>
					<div class="mx-auto w-full max-w-sm">
						<Drawer.Header>
							<Drawer.Title>Release 42</Drawer.Title>
							<Drawer.Description>Saved → Published in Colophon → Verified at 1 location.</Drawer.Description>
						</Drawer.Header>
						<Drawer.Footer>
							<Drawer.Close class={buttonVariants()}>Done</Drawer.Close>
						</Drawer.Footer>
					</div>
				</Drawer.Content>
			</Drawer.Root>
		</Specimen>

		<Specimen label="Popover">
			<Popover.Root>
				<Popover.Trigger class={buttonVariants({ variant: 'outline' })}>Share</Popover.Trigger>
				<Popover.Content class="w-72">
					<div class="grid gap-2">
						<p class="text-sm font-medium">Public API</p>
						<Input readonly value="/content/v1/my-site/production/articles" class="font-mono text-xs" />
					</div>
				</Popover.Content>
			</Popover.Root>
		</Specimen>

		<Specimen label="Hover card">
			<HoverCard.Root>
				<HoverCard.Trigger class="text-sm font-medium underline underline-offset-4">@ada</HoverCard.Trigger>
				<HoverCard.Content class="w-72">
					<div class="flex gap-3">
						<Avatar.Root><Avatar.Fallback>AL</Avatar.Fallback></Avatar.Root>
						<div>
							<p class="text-sm font-semibold">Ada Lovelace</p>
							<p class="text-sm text-muted-foreground">Publisher · 128 releases</p>
						</div>
					</div>
				</HoverCard.Content>
			</HoverCard.Root>
		</Specimen>

		<Specimen label="Dropdown menu">
			<DropdownMenu.Root>
				<DropdownMenu.Trigger class={buttonVariants({ variant: 'outline' })}>Actions</DropdownMenu.Trigger>
				<DropdownMenu.Content class="w-52">
					<DropdownMenu.Label>Entry</DropdownMenu.Label>
					<DropdownMenu.Item><FileTextIcon />Compare revisions</DropdownMenu.Item>
					<DropdownMenu.Item><RocketIcon />Add to changeset<DropdownMenu.Shortcut>⌘P</DropdownMenu.Shortcut></DropdownMenu.Item>
					<DropdownMenu.Separator />
					<DropdownMenu.Item variant="destructive">Unpublish</DropdownMenu.Item>
				</DropdownMenu.Content>
			</DropdownMenu.Root>
		</Specimen>

		<Specimen label="Context menu">
			<ContextMenu.Root>
				<ContextMenu.Trigger class="grid h-20 place-items-center rounded-2xl border border-dashed border-border text-sm text-muted-foreground">
					Right-click here
				</ContextMenu.Trigger>
				<ContextMenu.Content class="w-48">
					<ContextMenu.Item>Open</ContextMenu.Item>
					<ContextMenu.Item>Copy link</ContextMenu.Item>
					<ContextMenu.Separator />
					<ContextMenu.Item variant="destructive">Delete draft</ContextMenu.Item>
				</ContextMenu.Content>
			</ContextMenu.Root>
		</Specimen>
	</div>

	<div class="grid gap-10 lg:grid-cols-2">
		<Specimen label="Menubar">
			<Menubar.Root class="w-fit">
				<Menubar.Menu>
					<Menubar.Trigger>File</Menubar.Trigger>
					<Menubar.Content>
						<Menubar.Item>New entry<Menubar.Shortcut>⌘N</Menubar.Shortcut></Menubar.Item>
						<Menubar.Item>Save<Menubar.Shortcut>⌘S</Menubar.Shortcut></Menubar.Item>
					</Menubar.Content>
				</Menubar.Menu>
				<Menubar.Menu>
					<Menubar.Trigger>Publish</Menubar.Trigger>
					<Menubar.Content>
						<Menubar.Item>New changeset</Menubar.Item>
						<Menubar.Item>Releases</Menubar.Item>
					</Menubar.Content>
				</Menubar.Menu>
			</Menubar.Root>
		</Specimen>

		<Specimen label="Navigation menu">
			<NavigationMenu.Root>
				<NavigationMenu.List>
					<NavigationMenu.Item>
						<NavigationMenu.Trigger>Content</NavigationMenu.Trigger>
						<NavigationMenu.Content>
							<ul class="grid w-64 gap-1 p-2">
								<li><NavigationMenu.Link href="#data">Articles</NavigationMenu.Link></li>
								<li><NavigationMenu.Link href="#data">Pages</NavigationMenu.Link></li>
							</ul>
						</NavigationMenu.Content>
					</NavigationMenu.Item>
					<NavigationMenu.Item>
						<NavigationMenu.Link href="#overlays">Releases</NavigationMenu.Link>
					</NavigationMenu.Item>
				</NavigationMenu.List>
			</NavigationMenu.Root>
		</Specimen>

		<Specimen label="Command palette">
			<Command.Root class="rounded-2xl border border-border shadow-soft" disableInitialScroll>
				<Command.Input placeholder="Search titles or jump to…" />
				<Command.List>
					<Command.Empty>No results.</Command.Empty>
					<Command.Group heading="Go to">
						<Command.Item><FileTextIcon />Articles</Command.Item>
						<Command.Item><RocketIcon />Releases</Command.Item>
						<Command.Item><SettingsIcon />Settings</Command.Item>
					</Command.Group>
				</Command.List>
			</Command.Root>
		</Specimen>

		<Specimen label="Toasts">
			<div class="flex flex-wrap gap-3">
				<Button variant="outline" onclick={() => toast.success('Saved draft')}>Success</Button>
				<Button variant="outline" onclick={() => toast('Release 42 committed', { description: 'Website update pending at 1 location.' })}>
					With detail
				</Button>
				<Button variant="outline" onclick={() => toast.error("Couldn't publish: another release committed first")}>Error</Button>
			</div>
		</Specimen>
	</div>
</div>
