<!--
  A build-log post as a card, after the reference's blog: `wide` is the featured row (cover left,
  label, title, lead, byline and kind), `small` is a cover over the title and its kind. The cover
  grows a little on hover, like the reference's photographs.
-->
<script lang="ts">
	import Mark from './Mark.svelte';
	import PostCover from './PostCover.svelte';
	import { KIND, longDate } from '#lib/blog.ts';

	interface Summary {
		slug: string;
		title: string;
		shortTitle: string;
		description: string;
		date: string;
		kind: 'release' | 'experiment' | 'failure';
		milestone: string;
		step: string;
	}
	let { post, variant = 'small', label = 'Latest', heading = 'h3' }: { post: Summary; variant?: 'wide' | 'small'; label?: string; heading?: 'h2' | 'h3' } = $props();
</script>

{#if variant === 'wide'}
	<a href="/blog/{post.slug}/" class="glass flex flex-col gap-6 rounded-2xl p-2 outline-none focus-visible:ring-2 focus-visible:ring-ring tb:flex-row tb:gap-8 tb:pr-8">
		<div class="h-64 shrink-0 overflow-hidden rounded-xl tb:h-[29rem] tb:w-[45%] dt:w-[31.25rem]"><PostCover step={post.step} milestone={post.milestone} large /></div>
		<div class="flex flex-col justify-between gap-8 px-4 pb-4 tb:px-0 tb:py-6">
			<div>
				<span class="post-label inline-flex rounded-full px-3 pt-[9px] pb-1.5 text-[0.9375rem] leading-[1.25] font-semibold text-white uppercase" style="background: var(--label-latest)">{label}</span>
				<svelte:element this={heading} class="t-h3 mt-6">{post.shortTitle}</svelte:element>
				<p class="t-body-lg mt-4 line-clamp-4">{post.description}</p>
			</div>
			<div class="flex items-center justify-between gap-4">
				<span class="flex items-center gap-4">
					<span class="grid size-[3.25rem] shrink-0 place-items-center rounded-full bg-card shadow-[var(--shadow-icon)]"><Mark size={20} /></span>
					<span>
						<span class="block text-base leading-6 font-medium">The build log · step {post.step}</span>
						<time class="t-small block" datetime={post.date}>{longDate(post.date)}</time>
					</span>
				</span>
				<span class="kind t-label rounded-full px-3 pt-[9px] pb-2 text-white" style="background: var(--label-{post.kind})">{KIND[post.kind].word}</span>
			</div>
		</div>
	</a>
{:else}
	<a href="/blog/{post.slug}/" class="group flex flex-col gap-4 rounded-xl outline-none focus-visible:ring-2 focus-visible:ring-ring">
		<div class="aspect-[341/265] overflow-hidden rounded-xl"><PostCover step={post.step} milestone={post.milestone} /></div>
		<div class="flex items-start justify-between gap-4">
			<span class="min-w-0">
				<svelte:element this={heading} class="t-h6 line-clamp-2">{post.shortTitle}</svelte:element>
				<time class="t-small mt-1 block" datetime={post.date}>{longDate(post.date)} · step {post.step}</time>
			</span>
			<span class="kind t-label shrink-0 rounded-full px-3 pt-[9px] pb-2 text-white" style="background: var(--label-{post.kind})">{KIND[post.kind].word}</span>
		</div>
	</a>
{/if}
