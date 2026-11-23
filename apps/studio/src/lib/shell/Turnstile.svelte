<!--
  Cloudflare Turnstile's widget for the sign-up form (D-235). Rendered only when the server
  publishes a site key; the script loads once, on demand, and nothing else on the page depends on
  it. A token is good for one sign-up, so the parent calls `reset()` after every attempt.
-->
<script lang="ts" module>
	interface TurnstileApi {
		render(element: HTMLElement, options: Record<string, unknown>): string;
		reset(widget: string): void;
		remove(widget: string): void;
	}

	const SCRIPT = 'https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit';
	let loading: Promise<TurnstileApi> | null = null;

	function load(): Promise<TurnstileApi> {
		loading ??= new Promise<TurnstileApi>((resolve, reject) => {
			const script = document.createElement('script');
			script.src = SCRIPT;
			script.async = true;
			script.onload = () => {
				const api = (window as unknown as { turnstile?: TurnstileApi }).turnstile;
				if (api) resolve(api);
				else reject(new Error('the check did not load'));
			};
			script.onerror = () => {
				loading = null;
				reject(new Error('the check did not load'));
			};
			document.head.append(script);
		});
		return loading;
	}
</script>

<script lang="ts">
	import { onMount } from 'svelte';

	let { siteKey, token = $bindable('') }: { siteKey: string; token?: string } = $props();

	let host: HTMLDivElement;
	let api: TurnstileApi | null = null;
	let widget: string | null = null;
	let failed = $state(false);

	export function reset() {
		token = '';
		if (api && widget !== null) api.reset(widget);
	}

	onMount(() => {
		let gone = false;
		load()
			.then((loaded) => {
				if (gone) return;
				api = loaded;
				widget = loaded.render(host, {
					sitekey: siteKey,
					theme: 'auto',
					callback: (value: string) => (token = value),
					'expired-callback': () => (token = ''),
					'error-callback': () => {
						token = '';
					}
				});
			})
			.catch(() => (failed = true));
		return () => {
			gone = true;
			if (api && widget !== null) api.remove(widget);
		};
	});
</script>

<div bind:this={host} data-testid="turnstile" class="min-h-[65px]"></div>
{#if failed}
	<p role="alert" class="text-sm text-destructive">The check that you're a person couldn't load. Reload the page to try again.</p>
{/if}
