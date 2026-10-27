<!--
  After the reference's "work from anywhere" frame: one large rounded picture with two tabs over
  its foot. "Studio" shows the Studio's editor on a desktop; "Your site" shows the reference site
  (apps/web) on a phone, with the marker every page carries and the footer it prints. Both are
  HTML illustrations of real states and words, not screenshots; the tabs follow the WAI-ARIA
  pattern, and the pictures cross-fade (opacity only).
-->
<script lang="ts">
	import BadgeCheckIcon from '@lucide/svelte/icons/badge-check';
	import BoldIcon from '@lucide/svelte/icons/bold';
	import CheckIcon from '@lucide/svelte/icons/check';
	import Heading2Icon from '@lucide/svelte/icons/heading-2';
	import ItalicIcon from '@lucide/svelte/icons/italic';
	import LinkIcon from '@lucide/svelte/icons/link';
	import ListIcon from '@lucide/svelte/icons/list';
	import Mark from '#lib/components/Mark.svelte';
	import { reveal } from '#lib/motion.ts';

	const VIEWS = [
		{ id: 'studio', label: 'Studio' },
		{ id: 'site', label: 'Your site' }
	] as const;
	type View = (typeof VIEWS)[number]['id'];
	let active = $state<View>('studio');
	const id = $props.id();
	let tabs: HTMLButtonElement[] = $state([]);

	function key(event: KeyboardEvent, index: number) {
		const next =
			event.key === 'ArrowRight' || event.key === 'ArrowLeft' ? 1 - index : event.key === 'Home' ? 0 : event.key === 'End' ? 1 : null;
		if (next === null) return;
		event.preventDefault();
		active = VIEWS[next]!.id;
		tabs[next]?.focus();
	}
</script>

<section aria-labelledby="devices" class="section shell" {@attach reveal}>
	<div class="mx-auto max-w-[50rem] text-center">
		<p class="t-eyebrow mb-5">One release, every screen</p>
		<h2 id="devices" class="t-h2">Write in the Studio,<br class="max-tb:hidden" /> read it on your site</h2>
	</div>

	<div class="showcase relative mt-14 overflow-hidden rounded-3xl" data-testid="device-showcase">
		{#each VIEWS as view (view.id)}
			<div
				role="tabpanel"
				id="{id}-{view.id}"
				aria-labelledby="{id}-tab-{view.id}"
				class="view absolute inset-0"
				data-on={active === view.id}
				inert={active !== view.id}
			>
				{#if view.id === 'studio'}
					<!-- The Studio on a desktop: the editor of one article, saved, and its release verified. -->
					<div class="stage studio-stage" aria-hidden="true">
						<div class="flex h-full overflow-hidden rounded-2xl bg-card shadow-[var(--shadow-float)]">
							<div class="flex w-44 shrink-0 flex-col gap-1 border-r border-border bg-background px-3 py-4 text-[0.8125rem] text-ink-2">
								<span class="flex items-center gap-2 px-1 pb-3 font-semibold text-foreground"><Mark size={14} />Demo project</span>
								<span class="rounded-lg px-2 py-1.5">Overview</span>
								<span class="px-2 pt-2 text-[0.6875rem] text-muted-foreground">Content</span>
								<span class="rounded-lg bg-accent px-2 py-1.5 font-medium text-foreground">Articles</span>
								<span class="px-2 pt-2 text-[0.6875rem] text-muted-foreground">Publishing</span>
								<span class="rounded-lg px-2 py-1.5">Changesets</span>
								<span class="rounded-lg px-2 py-1.5">Releases</span>
							</div>
							<div class="flex min-w-0 flex-1 flex-col">
								<div class="flex h-14 items-center gap-3 border-b border-border px-6 text-[0.8125rem]">
									<span class="text-muted-foreground">Articles / <span class="text-foreground">Spring menu</span></span>
									<span class="ml-auto inline-flex items-center gap-1 rounded-full bg-live-soft px-2.5 py-0.5 text-[0.75rem] font-medium text-live-ink"><CheckIcon class="size-3" />Saved · version 7</span>
								</div>
								<div class="grid flex-1 grid-cols-[minmax(0,1fr)_14rem] gap-6 p-6">
									<div>
										<p class="text-[0.75rem] font-medium text-muted-foreground">Title</p>
										<p class="mt-1 rounded-xl border border-input px-3 py-2 text-base font-semibold">Spring menu</p>
										<p class="mt-4 text-[0.75rem] font-medium text-muted-foreground">Body</p>
										<div class="mt-1 rounded-xl border border-input">
											<div class="flex gap-1 border-b border-border px-2 py-1.5 text-muted-foreground">
												<span class="grid size-7 place-items-center rounded-md"><BoldIcon class="size-3.5" /></span>
												<span class="grid size-7 place-items-center rounded-md"><ItalicIcon class="size-3.5" /></span>
												<span class="grid size-7 place-items-center rounded-md bg-accent text-foreground"><Heading2Icon class="size-3.5" /></span>
												<span class="grid size-7 place-items-center rounded-md"><ListIcon class="size-3.5" /></span>
												<span class="grid size-7 place-items-center rounded-md"><LinkIcon class="size-3.5" /></span>
											</div>
											<div class="space-y-2 px-4 py-3 text-[0.8125rem] leading-relaxed text-ink-2">
												<p class="text-[0.9375rem] font-semibold text-foreground">This week’s soup</p>
												<p>Pea and mint, with bread from the bakery next door.</p>
												<p class="h-2 w-11/12 rounded-full bg-muted"></p>
												<p class="h-2 w-8/12 rounded-full bg-muted"></p>
											</div>
										</div>
										<p class="mt-3 text-[0.75rem] text-muted-foreground">A copy of unsaved edits stays on this device.</p>
									</div>
									<div class="space-y-3">
										<div class="rounded-xl border border-border p-3">
											<p class="text-[0.75rem] font-medium text-muted-foreground">Revisions</p>
											<ul class="mt-2 space-y-1.5 text-[0.8125rem]">
												<li class="flex justify-between"><span>version 7</span><span class="text-muted-foreground">current</span></li>
												<li class="flex justify-between"><span>version 6</span><span class="text-muted-foreground">compare</span></li>
												<li class="flex justify-between"><span>version 5</span><span class="text-muted-foreground">compare</span></li>
											</ul>
										</div>
										<div class="rounded-xl border border-border p-3">
											<p class="text-[0.75rem] font-medium text-muted-foreground">Live in release 42</p>
											<p class="mt-2 inline-flex items-center gap-1 rounded-full bg-live-soft px-2 py-0.5 text-[0.75rem] font-medium text-live-ink">
												<BadgeCheckIcon class="size-3" />Verified at 1 location
											</p>
										</div>
									</div>
								</div>
							</div>
						</div>
					</div>
				{:else}
					<!-- The reference site on a phone: plain HTML built from release 42, with its marker. -->
					<div class="stage site-stage" aria-hidden="true">
						<div class="phone rounded-[44px] bg-[#1a1615] p-2.5 shadow-[var(--shadow-float)]">
							<div class="flex h-full flex-col overflow-hidden rounded-[36px] bg-white text-[#1a1615]">
								<div class="flex items-center justify-between px-6 pt-3 text-[0.6875rem] font-semibold"><span>9:41</span><span class="h-5 w-20 rounded-full bg-[#1a1615]"></span><span>5G</span></div>
								<p class="mt-3 border-b border-[#e4e2e2] px-5 pb-2 font-mono text-[0.6875rem] text-[#5f5a58]">your-site.example/articles/spring-menu/</p>
								<div class="flex gap-3 px-5 pt-3 text-[0.8125rem] text-[#1a1615] underline underline-offset-2"><span>Home</span><span>Colophon</span></div>
								<div class="flex-1 px-5 pt-4">
									<p class="text-[1.375rem] leading-tight font-semibold">Spring menu</p>
									<p class="mt-2 text-[0.9375rem] font-semibold">This week’s soup</p>
									<p class="mt-1 text-[0.8125rem] leading-relaxed text-[#453f3d]">Pea and mint, with bread from the bakery next door.</p>
									<p class="mt-3 h-2 w-11/12 rounded-full bg-[#f0eae5]"></p>
									<p class="mt-2 h-2 w-9/12 rounded-full bg-[#f0eae5]"></p>
									<p class="mt-2 h-2 w-10/12 rounded-full bg-[#f0eae5]"></p>
								</div>
								<p class="border-t border-[#e4e2e2] px-5 py-3 text-[0.6875rem] leading-snug text-[#5f5a58]">Release 42, built 2026-10-27T09:14:03.000Z.</p>
							</div>
						</div>
						<div class="probe rounded-2xl bg-card p-3 shadow-[var(--shadow-float)]">
							<p class="font-mono text-[0.6875rem] text-muted-foreground">&lt;meta name="colophon-generation" content="42"&gt;</p>
							<p class="mt-2 inline-flex items-center gap-1 rounded-full bg-live-soft px-2 py-0.5 text-[0.75rem] font-medium text-live-ink">
								<BadgeCheckIcon class="size-3" />Verified at 1 location
							</p>
						</div>
					</div>
				{/if}
				<p class="sr-only">
					{#if view.id === 'studio'}
						Illustration: the Studio’s editor on a desktop, with the article Spring menu saved as version 7, its earlier versions to compare, and
						its release, 42, verified at 1 location.
					{:else}
						Illustration: the same article on a phone, on a site built from release 42; the page carries the marker colophon-generation 42 that
						the probe found, and its footer names the release.
					{/if}
				</p>
			</div>
		{/each}

		<div role="tablist" aria-label="Where to look" class="tabs absolute bottom-8 left-1/2 flex -translate-x-1/2 gap-3 rounded-[40px] p-2">
			{#each VIEWS as view, index (view.id)}
				<button
					bind:this={tabs[index]}
					type="button"
					role="tab"
					id="{id}-tab-{view.id}"
					aria-selected={active === view.id}
					aria-controls="{id}-{view.id}"
					tabindex={active === view.id ? 0 : -1}
					onclick={() => (active = view.id)}
					onkeydown={(event) => key(event, index)}
					class="pill pill-small pill-tertiary tab"
				>
					<span class="roll"><span>{view.label}</span><span aria-hidden="true">{view.label}</span></span>
				</button>
			{/each}
		</div>
	</div>
</section>

<style>
	/* 1,072 by 700 on a desktop, the full width by 700 on a tablet, by 400 on a phone. */
	.showcase {
		height: 400px;
		background: var(--visual-ground);
	}
	@media (min-width: 50.625rem) {
		.showcase {
			height: 700px;
		}
	}
	.view {
		transition: opacity 500ms cubic-bezier(0.22, 1, 0.36, 1);
	}
	.view[data-on='false'] {
		opacity: 0;
	}

	/* The pictures are drawn at a fixed size and scaled to the frame. */
	.stage {
		position: absolute;
		left: 50%;
		top: 50%;
		transform-origin: 50% 50%;
	}
	.studio-stage {
		width: 880px;
		height: 540px;
		transform: translate(-50%, calc(-50% - 44px)) scale(0.37);
	}
	.site-stage {
		width: 520px;
		height: 560px;
		transform: translate(-50%, calc(-50% - 44px)) scale(0.48);
	}
	@media (min-width: 50.625rem) {
		.studio-stage {
			transform: translate(-50%, calc(-50% - 50px)) scale(0.74);
		}
		.site-stage {
			transform: translate(-50%, calc(-50% - 50px)) scale(0.92);
		}
	}
	@media (min-width: 75rem) {
		.studio-stage {
			transform: translate(-50%, calc(-50% - 50px)) scale(1);
		}
		.site-stage {
			transform: translate(-50%, calc(-50% - 50px)) scale(0.92);
		}
	}
	.phone {
		position: absolute;
		left: 120px;
		top: 0;
		width: 280px;
		height: 560px;
	}
	.probe {
		position: absolute;
		right: 0;
		top: 300px;
		width: 230px;
	}

	/* The tabs sit on a glass bar, like the reference's. */
	.tabs {
		background: var(--nav-glass);
		box-shadow: inset 0 0 0 1px var(--nav-edge);
		-webkit-backdrop-filter: blur(10px);
		backdrop-filter: blur(10px);
	}
	.tab[aria-selected='true'] {
		background: var(--primary);
		color: var(--primary-foreground);
	}
	.tab[aria-selected='false'] {
		box-shadow: inset 0 0 0 1px var(--nav-edge);
	}
	.tab:focus-visible {
		box-shadow:
			0 0 0 2px var(--background),
			0 0 0 4px var(--ring);
	}
</style>
