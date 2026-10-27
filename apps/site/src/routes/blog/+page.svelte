<!--
  The build log, after the reference's blog index: the band with the newest post as a wide card,
  then every other post as a card, grouped by milestone.
-->
<script lang="ts">
	import RssIcon from '@lucide/svelte/icons/rss';
	import PageIntro from '#lib/components/PageIntro.svelte';
	import Pill from '#lib/components/Pill.svelte';
	import PostCard from '#lib/components/PostCard.svelte';
	import Seo from '#lib/components/Seo.svelte';
	import { MILESTONE_NAMES } from '#lib/blog.ts';
	import { reveal } from '#lib/motion.ts';
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
		One post per checkpoint, mistakes included. {data.posts.length} posts so far, first published at
		<a class="text-link" href={BUILD_LOG}>404human.dev/colophon</a>; each page here links its original.
	</p>
	{#snippet aside()}
		<Pill label="RSS feed" href="/blog/rss.xml" variant="tertiary">
			{#snippet icon()}<RssIcon class="size-4" aria-hidden="true" />{/snippet}
		</Pill>
	{/snippet}
	{#snippet below()}
		{#if latest}<div data-testid="blog-post"><PostCard post={latest} variant="wide" heading="h2" /></div>{/if}
	{/snippet}
</PageIntro>

<div class="shell">
	{#each groups as [milestone, posts] (milestone)}
		<section aria-labelledby="milestone-{milestone}" class="mt-20 dt:mt-24" {@attach reveal}>
			<div class="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
				<h2 id="milestone-{milestone}" class="t-h5">{milestone} · {MILESTONE_NAMES[milestone] ?? milestone}</h2>
				<span class="t-small">{posts.length} {posts.length === 1 ? 'post' : 'posts'}</span>
			</div>
			<ul class="mt-8 grid gap-x-6 gap-y-10 tb:grid-cols-2 dt:grid-cols-3">
				{#each posts as post (post.slug)}
					<li data-testid="blog-post"><PostCard {post} dated /></li>
				{/each}
			</ul>
		</section>
	{/each}
</div>
