<!--
  A failed read, said plainly (D-059, rule 3): what happened and what to do, with Sign in when
  that's the problem and Try again when trying again can help. A role that can't see something is
  told so, not shown a bare refusal. The server's words are in the Technical details.
-->
<script lang="ts">
	import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
	import * as Alert from '@colophon/ui/ui/alert';
	import { Button } from '@colophon/ui/ui/button';
	import { ApiError, signInUrl } from '#lib/api.ts';
	import { describe } from '#lib/errors.ts';
	import TechnicalDetails from './TechnicalDetails.svelte';

	let {
		error,
		what = 'this',
		retry
	}: {
		error: Error;
		/** What failed to load, for people: "publish requests", "Articles". */
		what?: string;
		/** Asks again; offered when another try can help. */
		retry?: () => unknown;
	} = $props();

	const said = $derived(describe(error, `see ${what}`));
	const forbidden = $derived(error instanceof ApiError && error.status === 403);
	const title = $derived(
		forbidden ? "Your role can't see this" : error instanceof ApiError && error.failure === 'retry' ? `Couldn't load ${what}` : said.title
	);
</script>

<Alert.Root variant="destructive" role="alert" data-testid="problem">
	<CircleAlertIcon />
	<Alert.Title>{title}</Alert.Title>
	<Alert.Description>
		<p>
			{said.text}
			{#if error instanceof ApiError && error.failure === 'signed-out'}
				<a class="underline" href={signInUrl(window.location.pathname)} data-sveltekit-reload>Sign in</a>
			{/if}
		</p>
		{#if retry && error instanceof ApiError && error.failure === 'retry'}
			<Button class="mt-2" variant="outline" size="sm" onclick={() => retry?.()}>Try again</Button>
		{/if}
		<TechnicalDetails class="mt-2"><p class="break-words">{said.technical}</p></TechnicalDetails>
	</Alert.Description>
</Alert.Root>
