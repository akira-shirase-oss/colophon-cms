<!--
  The site canvas (W.2): a page's draft — sections, menu and theme — rendered by the shared
  site-renderer inside a same-origin iframe, so what the editor shows is the document the site
  build emits. Every section carries its index; clicking one hands it back for the form to open.
  It is the draft being edited, not what readers see: publishing is what ships it.
-->
<script lang="ts">
	import { createQuery } from '@tanstack/svelte-query';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { pageHtml, renderNav, renderSections, SITE_CSS, siteCss, type NavItem, type ThemeContent } from '@colophon/site-renderer';
	import { type Content, getEntry, listEntries } from './api.ts';

	let {
		project,
		content,
		onsection
	}: {
		project: string;
		/** The draft's content, rebuilt live as fields change. */
		content: Content;
		/** A section was clicked: its place in the sections list. */
		onsection?: (index: number) => void;
	} = $props();

	// The other entries the canvas needs, fetched once each: the theme as the stylesheet, the
	// menu as the header's links (page links resolve through one draft read each — a summary's
	// list doesn't carry content).
	const theme = createQuery(() => ({
		queryKey: ['site-canvas', project, 'theme'],
		queryFn: async () => {
			const entries = await listEntries(project, 'theme');
			const first = entries[0];
			if (first === undefined) return null;
			const loaded = await getEntry(project, 'theme', first.id);
			return loaded.draft.content as ThemeContent;
		}
	}));
	const navigation = createQuery(() => ({
		queryKey: ['site-canvas', project, 'navigation'],
		queryFn: async () => {
			const entries = await listEntries(project, 'navigation');
			const first = entries[0];
			if (first === undefined) return [] as NavItem[];
			const loaded = await getEntry(project, 'navigation', first.id);
			const items = (loaded.draft.content['items'] ?? []) as { label?: string; page?: string; url?: string }[];
			const links: NavItem[] = [];
			for (const item of items) {
				let href = item.url;
				if (href === undefined && item.page !== undefined) {
					const linked = await getEntry(project, 'pages', item.page);
					const slug = linked.draft.content['slug'];
					href = typeof slug === 'string' ? `/${slug}` : `/${item.page}`;
				}
				if (item.label !== undefined && href !== undefined) links.push({ label: item.label, href });
			}
			return links;
		}
	}));

	// The rendered document, rebuilt a breath after the last keystroke so typing doesn't make the
	// frame flicker. A block the renderer can't read isn't hidden: the message takes the canvas's
	// place instead of a half page.
	let markup = $state('');
	let problem = $state('');
	$effect(() => {
		const draft = content;
		const themeContent = theme.data;
		const links = navigation.data;
		if (themeContent === undefined || themeContent === null || links === undefined) return;
		const timer = setTimeout(() => {
			try {
				const description = (draft['seo'] as { description?: string } | undefined)?.description;
				markup = pageHtml({
					title: String(draft['title'] ?? 'Untitled'),
					...(description === undefined ? {} : { description }),
					head: `<style>${SITE_CSS}${siteCss(themeContent)}</style>\n`,
					nav: renderNav(links),
					body: renderSections(draft['sections'], { assetUrl: (id) => `/assets/${id}`, annotate: true })
				});
				problem = '';
			} catch (error) {
				problem = error instanceof Error ? error.message : String(error);
			}
		}, 200);
		return () => clearTimeout(timer);
	});

	// Click-to-edit: the frame is same-origin (srcdoc, our own renderer's output — no sandbox, or
	// the listener couldn't reach in). Every srcdoc swap reloads it, so the listener goes on at
	// `load`, when the new document is there.
	let frame = $state<HTMLIFrameElement | null>(null);
	function attach() {
		const doc = frame?.contentDocument;
		if (doc === null || doc === undefined) return;
		doc.addEventListener('click', (event) => {
			// The canvas is a picture of the page, not the page: links never navigate it.
			event.preventDefault();
			const section = (event.target as Element).closest('.site-section');
			const index = section?.getAttribute('data-i');
			if (index === null || index === undefined) return;
			onsection?.(Number(index));
		});
	}
</script>

{#if theme.data === undefined || navigation.data === undefined}
	<Skeleton class="h-96 w-full rounded-xl" />
{:else if theme.data === null}
	<p class="rounded-lg border border-border bg-card px-4 py-3 text-sm text-muted-foreground">No theme entry yet — the website starter makes one.</p>
{:else}
	<p class="mb-3 text-sm text-muted-foreground">
		This page as you're editing it — click a section to change it. Readers see it once it's published.
	</p>
	{#if problem !== ''}
		<p role="alert" class="rounded-lg border border-destructive/30 bg-destructive/5 px-4 py-3 text-sm text-destructive" data-testid="canvas-problem">
			This page can't be rendered: {problem}
		</p>
	{:else}
		<iframe bind:this={frame} onload={attach} title="The page as you're editing it" srcdoc={markup} class="h-[36rem] w-full rounded-xl border border-border bg-white" data-testid="site-canvas"></iframe>
	{/if}
{/if}
