<!--
  Real API answers, one tab each: their status lines, header names, error codes and messages are
  the server's (crates/colophon-api: delivery.rs, error.rs, entries.rs, changesets.rs); ids,
  hashes and ETags are shortened or illustrative. A tab's lines arrive one after another when it
  is chosen (not under reduced motion). The tabs follow the WAI-ARIA tabs pattern.
-->
<script lang="ts">
	import { inView } from '#lib/motion.ts';
	import StatusBadge from '#lib/components/StatusBadge.svelte';

	type Line = { t: 'cmd' | 'more' | 'note' | 'status' | 'header' | 'key' | 'blank' | 'body'; s: string; v?: string; n?: string };
	interface Pane {
		id: string;
		label: string;
		what: string;
		building?: string;
		lines: Line[];
	}

	const ENTRY = '"$CMS/content/v1/$PROJECT/production/articles/$ENTRY"';
	const SAVE = '"$CMS/api/v1/projects/$PROJECT/environments/production/collections/articles/entries/$ENTRY"';
	const PUBLISH = '"$CMS/api/v1/projects/$PROJECT/environments/production/changesets/$CS/publish"';
	const ERR = (code: string, message: string, details: string) => [
		{ t: 'blank', s: '' },
		{ t: 'body', s: `{"error": {"code": "${code}",` },
		{ t: 'body', s: `  "message": "${message}",` },
		...(details ? [{ t: 'body', s: `  "details": ${details},` } as Line] : []),
		{ t: 'body', s: '  "request_id": "0192e7ca-…"}}' }
	] as Line[];

	const PANES: Pane[] = [
		{
			id: 'read',
			label: 'Read',
			what: 'Every answer names the release it was read at, and caches by it.',
			lines: [
				{ t: 'cmd', s: `curl -i ${ENTRY}` },
				{ t: 'status', s: 'HTTP/1.1 200 OK' },
				{ t: 'key', s: 'colophon-generation', v: '42' },
				{ t: 'key', s: 'etag', v: '"8f14e45fceea167a5a36dedd4bea2543"' },
				{ t: 'header', s: 'cache-control', v: 'public, max-age=0, must-revalidate, s-maxage=30' },
				{ t: 'header', s: 'colophon-cache', v: 'hit' },
				{ t: 'header', s: 'cache-tag', v: 'colophon:p:…, colophon:p:…:e:…, …:c:articles, …:n:…' },
				{ t: 'blank', s: '' },
				{ t: 'body', s: '{"data": {"title": "Spring menu", "content_hash": "sha256:9f2c…e41a",' },
				{ t: 'body', s: '          "published_in": 42, …}}' }
			]
		},
		{
			id: 'revalidate',
			label: 'Revalidate',
			what: 'Release 43 didn’t change this entry, so its ETag still matches: 304, no body.',
			lines: [
				{ t: 'cmd', s: `curl -i ${ENTRY} \\` },
				{ t: 'more', s: `     -H 'If-None-Match: "8f14e45fceea167a5a36dedd4bea2543"'` },
				{ t: 'status', s: 'HTTP/1.1 304 Not Modified' },
				{ t: 'key', s: 'colophon-generation', v: '43' },
				{ t: 'key', s: 'etag', v: '"8f14e45fceea167a5a36dedd4bea2543"' },
				{ t: 'header', s: 'colophon-cache', v: 'hit' },
				{ t: 'blank', s: '' },
				{ t: 'note', s: '# pinned reads: ?generation=40 answers as release 40 was (s-maxage=300)' }
			]
		},
		{
			id: 'gone',
			label: 'Taken down',
			what: 'An unpublished entry is refused at every release, and never cached.',
			lines: [
				{ t: 'cmd', s: 'curl -i "$CMS/content/v1/$PROJECT/production/articles/$ENTRY?generation=40"' },
				{ t: 'status', s: 'HTTP/1.1 410 Gone' },
				{ t: 'key', s: 'cache-control', v: 'no-store' },
				...ERR('gone', 'this was published and has been taken down', '')
			]
		},
		{
			id: 'stale',
			label: 'Stale save',
			what: 'A save names the version it was made on; an older one is refused, never merged.',
			lines: [
				{ t: 'cmd', s: `curl -i -X PUT ${SAVE} \\` },
				{ t: 'more', s: `     -H "Authorization: Bearer $TOKEN" -H 'If-Match: "0192e7b1-…"' \\` },
				{ t: 'more', s: `     -H "Idempotency-Key: $(uuidgen)" -H 'Content-Type: application/json' -d @draft.json` },
				{ t: 'status', s: 'HTTP/1.1 409 Conflict' },
				{ t: 'key', s: 'etag', v: '"0192e7c9-…"', n: '   ← the current version' },
				...ERR(
					'conflict',
					'the draft has changed since that revision; load the current one and try again',
					'{"reason": "stale_revision", "pointer": ""}'
				)
			]
		},
		{
			id: 'race',
			label: 'Race to publish',
			what: 'Two publishers on release 41: one commits release 42, the other is told so.',
			lines: [
				{ t: 'cmd', s: `curl -i -X POST ${PUBLISH} \\` },
				{ t: 'more', s: `     -H "Authorization: Bearer $TOKEN" -H 'If-Match: "sha256:4be1…07c2"' \\` },
				{ t: 'more', s: `     -H "Idempotency-Key: $(uuidgen)" -d '{"base_generation": 41}'` },
				{ t: 'status', s: 'HTTP/1.1 409 Conflict' },
				{ t: 'key', s: 'colophon-generation', v: '42' },
				...ERR(
					'conflict',
					'another release committed first: the environment is now at generation 42; look again, then publish',
					'{"reason": "generation_moved", "pointer": ""}'
				)
			]
		},
		{
			id: 'slug',
			label: 'Slug taken',
			what: 'URL names are unique per collection, enforced by PostgreSQL; the refusal names the holder.',
			lines: [
				{ t: 'cmd', s: `curl -i -X PUT ${SAVE} … -d '{"slug": "spring-menu", …}'` },
				{ t: 'status', s: 'HTTP/1.1 409 Conflict' },
				...ERR(
					'conflict',
					'\\"spring-menu\\" is already the slug of another entry (published)',
					'{"reason": "slug_taken", "pointer": "/slug",\n               "entries": [{"entry": "0192…", "title": "Spring menu", "state": "published"}]}'
				)
			]
		}
	];

	let active = $state(PANES[0]!.id);
	const pane = $derived(PANES.find((candidate) => candidate.id === active) ?? PANES[0]!);
	const id = $props.id();
	let tabs: HTMLButtonElement[] = $state([]);

	function key(event: KeyboardEvent, index: number) {
		const last = PANES.length - 1;
		const next =
			event.key === 'ArrowRight' ? (index === last ? 0 : index + 1)
			: event.key === 'ArrowLeft' ? (index === 0 ? last : index - 1)
			: event.key === 'Home' ? 0
			: event.key === 'End' ? last
			: null;
		if (next === null) return;
		event.preventDefault();
		active = PANES[next]!.id;
		tabs[next]?.focus();
	}
</script>

<div class="overflow-hidden glass rounded-3xl" {@attach inView()}>
	<div role="tablist" aria-label="API answers" class="flex gap-1 overflow-x-auto border-b border-border/70 bg-muted/50 p-1.5">
		{#each PANES as candidate, index (candidate.id)}
			<button
				bind:this={tabs[index]}
				type="button"
				role="tab"
				id="{id}-tab-{candidate.id}"
				aria-selected={active === candidate.id}
				aria-controls="{id}-panel"
				tabindex={active === candidate.id ? 0 : -1}
				onclick={() => (active = candidate.id)}
				onkeydown={(event) => key(event, index)}
				class="inline-flex shrink-0 items-center gap-1.5 rounded-full px-3.5 py-1.5 text-sm font-medium whitespace-nowrap text-muted-foreground outline-none hover:text-foreground focus-visible:ring-4 focus-visible:ring-ring/25 aria-selected:bg-card aria-selected:text-foreground aria-selected:shadow-hairline"
			>
				{candidate.label}{#if candidate.building}<span class="rounded-full bg-pending-soft px-1.5 text-[0.6875rem] text-pending-ink">{candidate.building}</span>{/if}
			</button>
		{/each}
	</div>
	<div role="tabpanel" id="{id}-panel" aria-labelledby="{id}-tab-{pane.id}" class="grid gap-0 lg:grid-cols-[minmax(0,1fr)_17rem]">
		<!-- A scrollable code block must be reachable by keyboard (axe: scrollable-region-focusable). -->
		<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
		<pre class="code overflow-x-auto px-5 py-5 font-mono text-[0.75rem] leading-relaxed" tabindex="0" aria-label="{pane.label}: the request and the answer">{#key pane.id}{#each pane.lines as line, index (index)}<span class="ln reveal" style="--i: {index}">{#if line.t === 'cmd'}<span class="text-muted-foreground">$ </span>{line.s}{:else if line.t === 'more' || line.t === 'body'}{line.s}{:else if line.t === 'note'}<span class="text-muted-foreground">{line.s}</span>{:else if line.t === 'status'}<span class="font-semibold">{line.s}</span>{:else if line.t === 'header' || line.t === 'key'}<span class="text-muted-foreground">{line.s}:</span> <span class={line.t === 'key' ? 'font-semibold text-brand-ink' : ''}>{line.v}</span>{#if line.n}<span class="text-muted-foreground">{line.n}</span>{/if}{/if}</span>{/each}{/key}</pre>
		<div class="flex flex-col gap-3 border-t border-border/70 p-5 lg:border-t-0 lg:border-l">
			<div><StatusBadge kind={pane.building ? 'building' : 'done'} at={pane.building ?? ''} /></div>
			<p class="text-[0.9375rem] leading-relaxed">{pane.what}</p>
			<p class="mt-auto text-xs text-muted-foreground">Header names, codes and messages are the server’s; ids and hashes are shortened.</p>
		</div>
	</div>
</div>

<style>
	.ln {
		display: block;
		min-height: 1lh;
		white-space: pre;
	}
	:global([data-run]) .ln {
		animation: line-in 260ms ease both;
		animation-delay: calc(var(--i) * 45ms);
	}
	@keyframes line-in {
		from {
			opacity: 0;
			transform: translateY(3px);
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.ln {
			animation: none !important;
		}
	}
</style>
