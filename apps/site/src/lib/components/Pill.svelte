<!--
  The site's button, after the reference's: a 55px pill whose label rolls up on hover (the line
  slides out and an identical copy slides in from below; the copy is hidden from assistive
  technology). A link when it has an href, a button otherwise.
-->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { HTMLAnchorAttributes, HTMLButtonAttributes } from 'svelte/elements';

	type Variant = 'primary' | 'tertiary' | 'secondary' | 'outline';
	let {
		label,
		href = '',
		variant = 'primary',
		small = false,
		external = false,
		class: className = '',
		icon,
		...rest
	}: {
		label: string;
		href?: string;
		variant?: Variant;
		small?: boolean;
		/** For links into the docs app, which this site's prerenderer must not follow. */
		external?: boolean;
		class?: string;
		icon?: Snippet;
	} & Omit<HTMLAnchorAttributes & HTMLButtonAttributes, 'class' | 'href'> = $props();

	const classes = $derived(`pill pill-${variant} ${small ? 'pill-small' : ''} ${className}`);
</script>

{#snippet inner()}
	<span class="roll"><span>{label}</span><span aria-hidden="true">{label}</span></span>
	{#if icon}{@render icon()}{/if}
{/snippet}

{#if href}
	<a {href} rel={external ? 'external' : undefined} class={classes} {...rest as HTMLAnchorAttributes}>{@render inner()}</a>
{:else}
	<button type="button" class={classes} {...rest as HTMLButtonAttributes}>{@render inner()}</button>
{/if}
