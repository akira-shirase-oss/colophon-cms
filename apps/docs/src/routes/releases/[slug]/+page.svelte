<!-- One milestone's release notes, from docs/releases/ (the template is docs/templates/release-notes.md). -->
<script lang="ts">
	import RepoDocPage from '#lib/components/RepoDocPage.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<RepoDocPage title={data.title} html={data.html} toc={data.toc} source={data.source} section="Release notes" capability={data.capability}>
	{#snippet explanation()}Its "All changes" section is the milestone's part of the changelog.{/snippet}
</RepoDocPage>
