<!--
  One build-log post, after the reference's article page: a centred title over the band, the lead
  and the byline, the step's cover, then the post in a 660px column, and three more posts.
-->
<script lang="ts">
	import ArrowLeftIcon from '@lucide/svelte/icons/arrow-left';
	import ArrowRightIcon from '@lucide/svelte/icons/arrow-right';
	import Mark from '#lib/components/Mark.svelte';
	import PostCard from '#lib/components/PostCard.svelte';
	import PostCover from '#lib/components/PostCover.svelte';
	import Seo from '#lib/components/Seo.svelte';
	import { KIND, MILESTONE_NAMES, longDate } from '#lib/blog.ts';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	const post = $derived(data.post);
</script>

<Seo title="{post.title} · Colophon build log" description={post.description} canonical={post.canonical} />

<article>
	<header class="hero-ground pt-32 tb:pt-[9.5rem] dt:pt-40">
		<div class="shell text-center">
			<nav aria-label="Breadcrumb">
				<ol class="t-small flex flex-wrap items-center justify-center gap-1.5">
					<li><a class="rounded underline-offset-4 hover:text-foreground hover:underline" href="/blog/">Blog</a></li>
					<li aria-hidden="true">/</li>
					<li>{post.milestone} · {MILESTONE_NAMES[post.milestone] ?? post.milestone}</li>
					<li aria-hidden="true">/</li>
					<li aria-current="page" class="text-foreground">{post.step}</li>
				</ol>
			</nav>
			<h1 class="t-h2 mx-auto mt-5 max-w-[47.5rem]">{post.title}</h1>
			<p class="t-lead mx-auto mt-4 max-w-[41.25rem] text-pretty">{post.description}</p>
			<div class="mt-6 flex items-center justify-center gap-3 text-left">
				<span class="grid size-12 shrink-0 place-items-center rounded-full bg-card shadow-[var(--shadow-icon)]"><Mark size={18} /></span>
				<span>
					<span class="block text-base leading-6 font-medium">The build log · step {post.step}</span>
					<span class="t-small block"><time datetime={post.date}>{longDate(post.date)}</time> · <a class="underline underline-offset-2 hover:text-foreground" href={post.canonical}>first published at 404human.dev</a></span>
				</span>
			</div>
		</div>
		<div class="shell mt-10">
			<div class="relative mx-auto h-56 max-w-[47.5rem] overflow-hidden rounded-t-2xl tb:h-[22rem]">
				<PostCover step={post.step} milestone={post.milestone} large />
				<span class="t-label absolute top-4 right-4 rounded-full px-3 pt-[9px] pb-2 text-white" style="background: var(--label-{post.kind})">{KIND[post.kind].word}</span>
			</div>
		</div>
	</header>

	<div class="shell">
		{#if post.glance}
			<section aria-labelledby="glance" class="glass mx-auto mt-12 max-w-[41.25rem] rounded-3xl p-6 tb:p-8">
				<h2 id="glance" class="t-eyebrow">At a glance</h2>
				<dl class="mt-5 grid gap-4 text-base leading-6">
					<div class="grid gap-1 tb:grid-cols-[5.5rem_minmax(0,1fr)] tb:gap-4">
						<dt class="font-medium">Goal</dt>
						<dd class="text-ink-2">{post.glance.goal}</dd>
					</div>
					<div class="grid gap-1 tb:grid-cols-[5.5rem_minmax(0,1fr)] tb:gap-4">
						<dt class="font-medium">Built</dt>
						<dd class="text-ink-2">{post.glance.built}</dd>
					</div>
					<div class="grid gap-1 tb:grid-cols-[5.5rem_minmax(0,1fr)] tb:gap-4">
						<dt class="font-medium">Result</dt>
						<dd>
							<ul class="flex flex-wrap gap-2">
								{#each post.glance.proof as item (item)}
									<li class="rounded-full bg-chip px-3 py-1 text-sm">{item}</li>
								{/each}
							</ul>
						</dd>
					</div>
					{#if post.glance.code}
						<div class="grid gap-1 tb:grid-cols-[5.5rem_minmax(0,1fr)] tb:gap-4">
							<dt class="font-medium">Code</dt>
							<dd><code class="font-mono text-sm break-all text-ink-2">{post.glance.code}</code></dd>
						</div>
					{/if}
				</dl>
			</section>
		{/if}

		<div class="prose-site mx-auto mt-12" data-testid="post-body">
			<!-- The build log's own Markdown, rendered at build time with raw HTML escaped (src/lib/server/markdown.ts). -->
			{@html post.html}
		</div>

		<p class="t-small mx-auto mt-12 max-w-[41.25rem] rounded-2xl bg-chip px-5 py-4">
			Copied unchanged from the build log, where it was
			<a class="text-link" href={post.canonical}>first published</a>. Numbers and statuses are as of the post’s date; the
			<a class="text-link" href="/changelog/">changelog</a> and the <a class="text-link" href="/roadmap/">roadmap</a> say where things stand now.
		</p>

		<nav aria-label="Previous and next" class="mx-auto mt-8 flex max-w-[41.25rem] justify-between gap-4 text-base">
			{#if data.older}
				<a href="/blog/{data.older.slug}/" class="inline-flex items-center gap-2 rounded font-medium underline-offset-4 hover:underline"><ArrowLeftIcon class="size-4" aria-hidden="true" />Previous · {data.older.step}</a>
			{:else}<span></span>{/if}
			{#if data.newer}
				<a href="/blog/{data.newer.slug}/" class="inline-flex items-center gap-2 rounded font-medium underline-offset-4 hover:underline">Next · {data.newer.step}<ArrowRightIcon class="size-4" aria-hidden="true" /></a>
			{/if}
		</nav>
	</div>

	<section aria-labelledby="more" class="section shell">
		<h2 id="more" class="t-h3 text-center">You might also like</h2>
		<ul class="mt-12 grid gap-6 dt:grid-cols-3">
			{#each data.related as other (other.slug)}
				<li><PostCard post={other} /></li>
			{/each}
		</ul>
	</section>
</article>
