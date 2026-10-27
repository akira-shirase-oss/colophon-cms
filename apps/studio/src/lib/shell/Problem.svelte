<!-- A failed read, said plainly, with a way to sign in when that's the problem. -->
<script lang="ts">
	import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
	import * as Alert from '@colophon/ui/ui/alert';
	import { ApiError, signInUrl } from '#lib/api.ts';

	let { error }: { error: Error } = $props();
</script>

<Alert.Root variant="destructive" role="alert">
	<CircleAlertIcon />
	<Alert.Title>{error instanceof ApiError && error.failure === 'not-found' ? 'Not found' : "Couldn't load this"}</Alert.Title>
	<Alert.Description>
		{error.message}
		{#if error instanceof ApiError && error.failure === 'signed-out'}
			<a class="underline" href={signInUrl(window.location.pathname)} data-sveltekit-reload>Sign in</a>
		{/if}
	</Alert.Description>
</Alert.Root>
