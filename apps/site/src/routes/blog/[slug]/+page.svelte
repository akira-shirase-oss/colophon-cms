<script lang="ts">
	import ArrowLeftIcon from '@lucide/svelte/icons/arrow-left';
	import ArrowRightIcon from '@lucide/svelte/icons/arrow-right';
	import ArrowUpRightIcon from '@lucide/svelte/icons/arrow-up-right';
	import { Badge } from '@colophon/ui/ui/badge';
	import Seo from '#lib/components/Seo.svelte';
	import { KIND, MILESTONE_NAMES, longDate } from '#lib/blog.ts';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	const post = $derived(data.post);
</script>

<Seo title="{post.title} · Colophon build log" description={post.description} canonical={post.canonical} />

<article class="pb-8">
	<header class="haze border-b border-border">
		<div class="shell max-w-4xl py-12 sm:py-16">
			<nav aria-label="Breadcrumb">
				<ol class="flex flex-wrap items-center gap-1.5 text-sm text-muted-foreground">
					<li><a class="rounded underline-offset-4 hover:text-foreground hover:underline" href="/blog/">Blog</a></li>
					<li aria-hidden="true">/</li>
					<li>{post.milestone} · {MILESTONE_NAMES[post.milestone] ?? post.milestone}</li>
					<li aria-hidden="true">/</li>
					<li aria-current="page" class="text-foreground">{post.step}</li>
				</ol>
			</nav>
			<h1 class="display-1 mt-6">{post.title}</h1>
			<p class="mt-5 text-lg leading-relaxed text-pretty text-muted-foreground">{post.description}</p>
			<div class="mt-6 flex flex-wrap items-center gap-x-4 gap-y-2 text-sm text-muted-foreground">
				<Badge variant={KIND[post.kind].variant}>{KIND[post.kind].word}</Badge>
				<time datetime={post.date}>{longDate(post.date)}</time>
				<a class="inline-flex items-center gap-1 rounded underline underline-offset-4 hover:text-foreground" href={post.canonical}>
					First published at 404human.dev<ArrowUpRightIcon class="size-3.5" aria-hidden="true" />
				</a>
			</div>
		</div>
	</header>

	<div class="shell max-w-4xl">
		{#if post.glance}
			<section aria-labelledby="glance" class="mt-10 rounded-3xl border border-border/70 bg-card p-6 shadow-soft sm:p-8">
				<h2 id="glance" class="eyebrow">At a glance</h2>
				<dl class="mt-4 grid gap-4 text-[0.9375rem]">
					<div class="grid gap-1 sm:grid-cols-[6rem_minmax(0,1fr)] sm:gap-4">
						<dt class="font-medium">Goal</dt>
						<dd class="text-muted-foreground">{post.glance.goal}</dd>
					</div>
					<div class="grid gap-1 sm:grid-cols-[6rem_minmax(0,1fr)] sm:gap-4">
						<dt class="font-medium">Built</dt>
						<dd class="text-muted-foreground">{post.glance.built}</dd>
					</div>
					<div class="grid gap-1 sm:grid-cols-[6rem_minmax(0,1fr)] sm:gap-4">
						<dt class="font-medium">Result</dt>
						<dd>
							<ul class="flex flex-wrap gap-2">
								{#each post.glance.proof as item (item)}
									<li class="rounded-full border border-border bg-background px-3 py-1 text-sm">{item}</li>
								{/each}
							</ul>
						</dd>
					</div>
					{#if post.glance.code}
						<div class="grid gap-1 sm:grid-cols-[6rem_minmax(0,1fr)] sm:gap-4">
							<dt class="font-medium">Code</dt>
							<dd><code class="font-mono text-sm break-all text-muted-foreground">{post.glance.code}</code></dd>
						</div>
					{/if}
				</dl>
			</section>
		{/if}

		<div class="prose-site mx-auto mt-12" data-testid="post-body">
			<!-- The build log's own Markdown, rendered at build time with raw HTML escaped (src/lib/server/markdown.ts). -->
			{@html post.html}
		</div>

		<p class="mx-auto mt-12 max-w-[68ch] rounded-2xl border border-border bg-card px-5 py-4 text-sm leading-relaxed text-muted-foreground">
			Copied unchanged from the build log, where it was
			<a class="text-link" href={post.canonical}>first published</a>. Numbers and statuses are as of the post’s date; the
			<a class="text-link" href="/changelog/">changelog</a> and the <a class="text-link" href="/roadmap/">roadmap</a> say where things stand now.
		</p>

		<nav aria-label="More from the build log" class="mt-12 grid gap-4 sm:grid-cols-2">
			{#if data.older}
				<a href="/blog/{data.older.slug}/" class="lift rounded-3xl border border-border/70 bg-card p-5 shadow-hairline outline-none focus-visible:ring-4 focus-visible:ring-ring/25">
					<span class="flex items-center gap-1.5 text-xs text-muted-foreground"><ArrowLeftIcon class="size-3.5" aria-hidden="true" />Previous · {data.older.step}</span>
					<span class="mt-2 block font-medium">{data.older.shortTitle}</span>
				</a>
			{:else}
				<span></span>
			{/if}
			{#if data.newer}
				<a href="/blog/{data.newer.slug}/" class="lift rounded-3xl border border-border/70 bg-card p-5 text-right shadow-hairline outline-none focus-visible:ring-4 focus-visible:ring-ring/25">
					<span class="flex items-center justify-end gap-1.5 text-xs text-muted-foreground">Next · {data.newer.step}<ArrowRightIcon class="size-3.5" aria-hidden="true" /></span>
					<span class="mt-2 block font-medium">{data.newer.shortTitle}</span>
				</a>
			{/if}
		</nav>
	</div>
</article>
