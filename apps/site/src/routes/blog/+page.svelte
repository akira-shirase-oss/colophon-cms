<script lang="ts">
	import ArrowRightIcon from '@lucide/svelte/icons/arrow-right';
	import RssIcon from '@lucide/svelte/icons/rss';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import PageIntro from '#lib/components/PageIntro.svelte';
	import Seo from '#lib/components/Seo.svelte';
	import { KIND, MILESTONE_NAMES, longDate } from '#lib/blog.ts';
	import { BUILD_LOG } from '#lib/site.ts';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const latest = $derived(data.posts[0]);
	const groups = $derived.by(() => {
		const byMilestone = new Map<string, typeof data.posts>();
		for (const post of data.posts.slice(1)) {
			const list = byMilestone.get(post.milestone) ?? [];
			list.push(post);
			byMilestone.set(post.milestone, list);
		}
		return [...byMilestone.entries()];
	});
</script>

<Seo
	title="Blog: the Colophon build log"
	description="Every step of Colophon CMS written up as it happened, mistakes included: the plan, each checkpoint, and what broke."
/>

<PageIntro badge="Build log" title="Every step, written up as it happened">
	<p>
		One post per checkpoint, mistakes included: what was built, how it was proven, and where it went wrong. {data.posts.length} posts so far, first
		published at <a class="text-link" href={BUILD_LOG}>404human.dev/colophon</a>; each page here links its original.
	</p>
	{#snippet aside()}
		<Button variant="outline" href="/blog/rss.xml"><RssIcon aria-hidden="true" />RSS feed</Button>
	{/snippet}
</PageIntro>

<div class="shell mt-12">
	{#if latest}
		<a
			href="/blog/{latest.slug}/"
			data-testid="blog-post"
			class="lift glow block overflow-hidden rounded-[2rem] p-3 outline-none focus-visible:ring-4 focus-visible:ring-ring/25 sm:p-4"
		>
			<article class="rounded-3xl bg-card p-6 shadow-float sm:p-10">
				<div class="flex flex-wrap items-center gap-2">
					<Badge variant="pending">Latest</Badge>
					<Badge variant={KIND[latest.kind].variant}>{KIND[latest.kind].word}</Badge>
					<span class="text-sm text-muted-foreground">{latest.milestone} · {latest.step} · <time datetime={latest.date}>{longDate(latest.date)}</time></span>
				</div>
				<h2 class="display-2 mt-5 max-w-3xl">{latest.title}</h2>
				<p class="mt-4 max-w-3xl text-[1.0625rem] leading-relaxed text-pretty text-muted-foreground">{latest.description}</p>
				<span class="mt-6 inline-flex items-center gap-1.5 text-sm font-medium">Read the post<ArrowRightIcon class="size-4" aria-hidden="true" /></span>
			</article>
		</a>
	{/if}

	{#each groups as [milestone, posts] (milestone)}
		<section aria-labelledby="milestone-{milestone}" class="mt-16">
			<div class="flex flex-wrap items-baseline gap-x-3 gap-y-1 border-b border-border pb-3">
				<h2 id="milestone-{milestone}" class="title">{milestone} · {MILESTONE_NAMES[milestone] ?? milestone}</h2>
				<span class="text-sm text-muted-foreground">{posts.length} {posts.length === 1 ? 'post' : 'posts'}</span>
			</div>
			<ul class="divide-y divide-border">
				{#each posts as post (post.slug)}
					<li data-testid="blog-post">
						<a
							href="/blog/{post.slug}/"
							class="group grid gap-2 rounded-2xl py-6 outline-none focus-visible:ring-4 focus-visible:ring-ring/25 sm:grid-cols-[9.5rem_minmax(0,1fr)] sm:gap-6"
						>
							<div class="flex flex-wrap items-center gap-2 text-sm text-muted-foreground sm:flex-col sm:items-start">
								<time datetime={post.date}>{longDate(post.date)}</time>
								<span class="flex items-center gap-2">
									<span class="figure font-medium text-foreground">{post.step}</span>
									<Badge variant={KIND[post.kind].variant}>{KIND[post.kind].word}</Badge>
								</span>
							</div>
							<div>
								<h3 class="text-[1.0625rem] leading-snug font-semibold underline-offset-4 group-hover:underline">{post.title}</h3>
								<p class="mt-1.5 line-clamp-2 text-[0.9375rem] leading-relaxed text-muted-foreground">{post.description}</p>
							</div>
						</a>
					</li>
				{/each}
			</ul>
		</section>
	{/each}
</div>
