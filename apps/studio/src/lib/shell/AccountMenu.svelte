<!--
  Who is signed in, and signing out, from any screen. Signing out ends the session on the server
  first; only then does the page reload at the front door, so nothing of the old session stays
  in memory. If the server can't be reached, it says so and stays signed in: it never claims a
  sign-out that didn't happen.
-->
<script lang="ts">
	import LayoutGridIcon from '@lucide/svelte/icons/layout-grid';
	import LogOutIcon from '@lucide/svelte/icons/log-out';
	import { resolve } from '$app/paths';
	import { createQuery } from '@tanstack/svelte-query';
	import { toast } from 'svelte-sonner';
	import { buttonVariants } from '@colophon/ui/ui/button';
	import * as DropdownMenu from '@colophon/ui/ui/dropdown-menu';
	import { ApiError, signOut } from '#lib/api.ts';
	import { meQuery } from '#lib/queries.ts';

	const who = createQuery(meQuery);
	const name = $derived(who.data?.display_name ?? who.data?.subject ?? '');
	const initials = $derived(
		name
			.split(/\s+/)
			.filter((word) => word !== '')
			.slice(0, 2)
			.map((word) => word.slice(0, 1).toUpperCase())
			.join('') || '?'
	);
	let busy = $state(false);

	async function leave() {
		busy = true;
		try {
			await signOut();
			window.location.assign(`${resolve('/')}?signed-out=1`);
		} catch (failure) {
			busy = false;
			const why =
				failure instanceof ApiError && failure.failure === 'retry'
					? "Colophon didn't answer. Check your connection and try again."
					: 'Reload the page and try again.';
			toast.error(`You're still signed in. ${why}`);
		}
	}
</script>

{#if who.data}
	<DropdownMenu.Root>
		<DropdownMenu.Trigger class="{buttonVariants({ variant: 'ghost', size: 'icon-sm' })} rounded-full" data-testid="account-menu">
			<span class="flex size-7 items-center justify-center rounded-full border border-border bg-secondary text-[0.6875rem] font-semibold" aria-hidden="true"
				>{initials}</span
			><span class="sr-only">Account: {name}</span>
		</DropdownMenu.Trigger>
		<DropdownMenu.Content align="end" class="w-60">
			<DropdownMenu.Label class="grid gap-0.5 font-normal">
				<span class="text-xs text-muted-foreground">Signed in as</span>
				<span class="truncate text-sm font-medium" data-user-content>{name}</span>
			</DropdownMenu.Label>
			<DropdownMenu.Separator />
			<DropdownMenu.Item>
				{#snippet child({ props })}<a {...props} href={resolve('/')}><LayoutGridIcon />Your work and projects</a>{/snippet}
			</DropdownMenu.Item>
			<DropdownMenu.Item onSelect={leave} disabled={busy} data-testid="sign-out"><LogOutIcon />{busy ? 'Signing out…' : 'Sign out'}</DropdownMenu.Item>
		</DropdownMenu.Content>
	</DropdownMenu.Root>
{/if}
