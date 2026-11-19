<!--
  A small pixel-art icon, the website's (its closing statement draws the same grids): a grid of colour letters
  turned into one path per colour. k outline, b blue, o orange, g green, p pink, c cream, . empty. Decorative
  (aria-hidden, never focusable). With `wiggle` it gives a small wiggle on hover and on tap, once, back to rest.
-->
<script lang="ts" module>
	export type PixelName = 'pen' | 'people' | 'stamp' | 'receipt' | 'globe' | 'blocks' | 'check' | 'empty';
</script>

<script lang="ts">
	let { name, size = '1.5rem', wiggle = false, class: className = '' }: { name: PixelName; size?: string; wiggle?: boolean; class?: string } = $props();

	const ART: Record<PixelName, string[]> = {
		pen: ['.........kkk', '........kpppk', '.......kooppk', '......koook.', '.....koook..', '....koook...', '...koook....', '..kcook.....', '.kccck......', '.kckk.......', 'kkk.........', '............'],
		people: ['..kkk...kkk..', '.kccck.kccck.', '.kccck.kccck.', '..kkk...kkk..', '.............', '..kkk...kkk..', '.kbbbk.kgggk.', 'kbbbbbkgggggk', 'kbbbbbkgggggk', 'kbbbbbkgggggk', 'kkkkkkkkkkkkk', '.............'],
		stamp: ['....kkkk....', '...kppppk...', '...kppppk...', '....kppk....', '....kppk....', '..kkkkkkkk..', '.kppppppppk.', '.kkkkkkkkkk.', '............', '..bbbbbbbb..', '..bbbbbbbb..', '............'],
		receipt: ['kkkkkkkkk', 'kccccccck', 'kcbbbbcck', 'kccccccck', 'kcbbbccck', 'kccccccck', 'kcooooock', 'kccccccck', 'kccgggcck', 'kccccccck', 'kckckckck', '.k.k.k.k.'],
		globe: ['...kkkkk...', '..kbbggbk..', '.kbgggbbbk.', 'kbbgggbbggk', 'kbbbggbbggk', 'kbbbbbbgggk', 'kbggbbbbbbk', 'kbgggbbbbbk', '.kgggbbbbk.', '..kbgbbbk..', '...kkkkk...', '...........'],
		// Content types: four blocks of a page, each its own kind
		blocks: ['kkkkkk.kkkkk', 'kbbbbk.kook.', 'kbbbbk.kook.', 'kbbbbk.kkkk.', 'kkkkkk......', '............', 'kkkkk.kkkkkk', 'kggk..kpppk.', 'kggk..kpppk.', 'kkkk..kkkkk.', '............', '............'],
		// A done step: a green tick in a ring
		check: ['...kkkkk...', '..kgggggk..', '.kgggggggk.', 'kgggggggcgk', 'kggggggccgk', 'kgcgggccggk', 'kgccgccgggk', 'kggcccggggk', '.kggcggggk.', '..kgggggk..', '...kkkkk...', '...........'],
		// An empty state: a blank page with a folded corner
		empty: ['kkkkkkk....', 'kcccccckk..', 'kcccccckck.', 'kcccccckkkk', 'kcbbbbbbcck', 'kccccccccck', 'kcbbbbcccck', 'kccccccccck', 'kcbbbbbbcck', 'kccccccccck', 'kkkkkkkkkkk', '...........']
	};
	const CLASS: Record<string, string> = { k: 'k', b: 'b', o: 'o', g: 'g', p: 'p', c: 'c' };

	const rows = $derived(ART[name]);
	const width = $derived(Math.max(...rows.map((row) => row.length)));
	const height = $derived(rows.length);
	// One path per colour, made of that colour's runs.
	const paths = $derived.by(() => {
		const out = new Map<string, string>();
		rows.forEach((row, y) => {
			let x = 0;
			while (x < row.length) {
				const c = row[x] ?? '.';
				let end = x;
				while (end < row.length && row[end] === c) end++;
				const cls = CLASS[c];
				if (cls) out.set(cls, `${out.get(cls) ?? ''}M${x} ${y}h${end - x}v1h-${end - x}z`);
				x = end;
			}
		});
		return [...out];
	});

	let tapped = $state(false);
</script>

<svg
	class="pixel {className}"
	class:v2-wiggle={wiggle}
	data-tap={tapped ? '' : undefined}
	viewBox="0 0 {width} {height}"
	style="height: {size}; width: calc({size} * {width / height})"
	aria-hidden="true"
	focusable="false"
	shape-rendering="crispEdges"
	onclick={() => {
		if (!wiggle) return;
		tapped = false;
		requestAnimationFrame(() => (tapped = true));
	}}
	onanimationend={() => (tapped = false)}
>
	{#each paths as [c, d] (c)}<path class={c} {d} />{/each}
</svg>

<style>
	.pixel {
		display: inline-block;
		flex-shrink: 0;
		overflow: visible;
	}
	.k {
		fill: var(--px-outline);
	}
	.b {
		fill: var(--px-blue);
	}
	.o {
		fill: var(--px-orange);
	}
	.g {
		fill: var(--px-green);
	}
	.p {
		fill: var(--px-pink);
	}
	.c {
		fill: var(--px-cream);
	}
</style>
