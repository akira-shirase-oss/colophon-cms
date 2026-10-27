<script lang="ts">
	import SaveIcon from '@lucide/svelte/icons/save';
	import BadgeCheckIcon from '@lucide/svelte/icons/badge-check';
	import GitCompareIcon from '@lucide/svelte/icons/git-compare';
	import HistoryIcon from '@lucide/svelte/icons/history';
	import PackageCheckIcon from '@lucide/svelte/icons/package-check';
	import RadarIcon from '@lucide/svelte/icons/radar';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import SearchIcon from '@lucide/svelte/icons/search';
	import ShieldCheckIcon from '@lucide/svelte/icons/shield-check';
	import UsersIcon from '@lucide/svelte/icons/users';
	import BentoCard from '#lib/bento/BentoCard.svelte';
	import CrashGraphic from '#lib/bento/CrashGraphic.svelte';
	import SeedGraphic from '#lib/bento/SeedGraphic.svelte';
	import PostCard from '#lib/components/PostCard.svelte';
	import Seo from '#lib/components/Seo.svelte';
	import Community from '#lib/home/Community.svelte';
	import DeviceShowcase from '#lib/home/DeviceShowcase.svelte';
	import EntriesPanel from '#lib/home/EntriesPanel.svelte';
	import FeatureBlock from '#lib/home/FeatureBlock.svelte';
	import HomeHero from '#lib/home/HomeHero.svelte';
	import MilestonePlans from '#lib/home/MilestonePlans.svelte';
	import QuoteSection from '#lib/home/QuoteSection.svelte';
	import ReleasePanel from '#lib/home/ReleasePanel.svelte';
	import TechMarquee from '#lib/home/TechMarquee.svelte';
	import { reveal } from '#lib/motion.ts';
	import { DESCRIPTION } from '#lib/site.ts';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	const [featured, ...recent] = $derived(data.posts);
</script>

<Seo title="Colophon: the open-source CMS that can prove what it published" description={DESCRIPTION} />

<HomeHero />

<TechMarquee />

<DeviceShowcase />

<!-- Two features, side by side with their pictures, the second one mirrored. -->
<section aria-label="Writing and publishing" class="section shell flex flex-col gap-20 tb:gap-24 dt:gap-[7.5rem]">
	<FeatureBlock
		id="writing"
		eyebrow="Writing"
		title="Edit with confidence"
		action={{ label: 'See every feature', href: '/product/' }}
		tags={[
			{ label: 'Autosave', icon: SaveIcon },
			{ label: 'Revisions', icon: HistoryIcon },
			{ label: 'Review diffs', icon: GitCompareIcon },
			{ label: 'Title search', icon: SearchIcon }
		]}
		alt="the Studio’s list of articles, searched by title, each with its state and revision, over a comparison of revisions 6 and 7 of Spring menu."
	>
		{#snippet lead()}
			<strong>Drafts that can’t be lost.</strong> Every save is a new revision that is never changed; unsaved words wait on your device, a save on top of
			someone else’s is refused instead of overwriting it, and reviewers see exactly what changed.
		{/snippet}
		{#snippet picture()}<EntriesPanel />{/snippet}
	</FeatureBlock>

	<FeatureBlock
		id="publishing"
		eyebrow="Publishing"
		title="Publish with proof"
		flip
		action={{ label: 'How publishing works', href: '/product/#publishing' }}
		tags={[
			{ label: 'Releases', icon: PackageCheckIcon },
			{ label: 'Receipts', icon: BadgeCheckIcon },
			{ label: 'Probes', icon: RadarIcon },
			{ label: 'CDN purge', icon: RefreshCwIcon }
		]}
		alt="release 42 with its entries, the location verified and the page hash it kept; its receipt from Saved to Verified at 1 location; and a probe finding the release’s marker on the live page."
	>
		{#snippet lead()}
			<strong>One number per publish.</strong> A publish commits one release in one transaction; webhooks and CDN purges run after it and survive a crash,
			and the receipt says verified only once a probe has seen the release on your live page.
		{/snippet}
		{#snippet picture()}<ReleasePanel />{/snippet}
	</FeatureBlock>
</section>

<!-- The guarantees: two wide cells with live pictures, three narrow ones. -->
<section aria-labelledby="promises" class="section shell" {@attach reveal}>
	<div class="mx-auto max-w-[50rem] text-center">
		<p class="t-eyebrow mb-5">What it guarantees</p>
		<h2 id="promises" class="t-h2">Built around what<br class="max-tb:hidden" /> is hard to fake</h2>
	</div>
	<div class="mt-10 grid gap-6 dt:mt-14 dt:grid-cols-6">
		<BentoCard
			kind="done"
			at="P1, P2"
			title="Kill it at any moment, lose nothing"
			alt="colophon-server is killed with kill -9 while version 7 is saving; it restarts, and the retry with the same key is saved once, with versions 5 and 6 untouched."
			class="dt:col-span-3"
		>
			{#snippet graphic()}<CrashGraphic />{/snippet}
			<strong>Fault tests F01 to F05</strong> on the real binary, and a crash loop with twelve SIGKILLs in every CI run.
		</BentoCard>
		<BentoCard
			kind="done"
			at="P1, P2"
			title="Every bug is a seed, and replays exactly"
			alt="a terminal runs the simulator twice with the same seed; both runs end on the same trace hash."
			class="dt:col-span-3"
		>
			{#snippet graphic()}<SeedGraphic />{/snippet}
			<strong>2,000 seeds on every pull request.</strong> A deterministic simulator of editors, crashes and lost answers; a failing seed replays step for step.
		</BentoCard>
		<BentoCard kind="done" at="P2" title="Approve the exact change" icon={ShieldCheckIcon} class="dt:col-span-2">
			An approval is bound to a hash of the versions; edit one afterwards and acting on it returns 409.
		</BentoCard>
		<BentoCard kind="planned" at="P4" title="Time travel" icon={HistoryIcon} class="dt:col-span-2">
			Reads pinned to any release work today; the release slider and rollback come in P4.
		</BentoCard>
		<BentoCard kind="building" at="R" title="Roles you define" icon={UsersIcon} class="dt:col-span-2">
			Roles are data now; custom roles, field-level rules, API tokens and an audit log come next.
		</BentoCard>
	</div>
</section>

<QuoteSection question={data.question} quotes={data.quotes} />

<MilestonePlans />

<!-- The build log: the newest post wide, then the three before it. -->
<section aria-labelledby="journal" class="section shell" {@attach reveal}>
	<div class="mx-auto max-w-[50rem] text-center">
		<p class="t-eyebrow mb-5">Build log</p>
		<h2 id="journal" class="t-h2">Every step,<br class="max-tb:hidden" /> written up as it happened</h2>
	</div>
	{#if featured}
		<div class="mt-10 dt:mt-14"><PostCard post={featured} variant="wide" /></div>
	{/if}
	<ul class="mt-6 grid gap-6 dt:grid-cols-3">
		{#each recent as post (post.slug)}
			<li><PostCard {post} /></li>
		{/each}
	</ul>
</section>

<Community />
