<script lang="ts">
	import ArrowUpRightIcon from '@lucide/svelte/icons/arrow-up-right';
	import { Badge } from '@colophon/ui/ui/badge';
	import Pill from '#lib/components/Pill.svelte';
	import PageIntro from '#lib/components/PageIntro.svelte';
	import SectionHeading from '#lib/components/SectionHeading.svelte';
	import Seo from '#lib/components/Seo.svelte';
	import { BUILD_LOG, GITHUB, GITHUB_FILE, QUICKSTART } from '#lib/site.ts';

	const STACK = [
		{ layer: 'Server', choice: 'Rust (edition 2024): Axum, Tokio, Tower, SQLx; one binary with API and worker roles' },
		{ layer: 'Data', choice: 'PostgreSQL 18, the only authority; Valkey, a disposable cache; S3-compatible object storage (Garage locally)' },
		{ layer: 'Frontends', choice: 'SvelteKit and TypeScript: the Studio, the reference site, and this website' },
		{ layer: 'Design system', choice: 'Colophon’s own: OKLCH tokens, Tailwind CSS v4, owned shadcn-svelte components on Bits UI, Inter (self-hosted)' },
		{ layer: 'Editor', choice: 'Tiptap / ProseMirror, stored as validated, versioned JSON, never HTML' },
		{ layer: 'Identity', choice: 'OpenID Connect, with Keycloak as the reference provider' },
		{ layer: 'Verification', choice: 'Unit and property tests, a deterministic simulator, real PostgreSQL per test, Vitest, Playwright, k6' }
	];

	const LOCAL = [
		{ command: 'scripts/stack.sh up', note: 'PostgreSQL, Valkey, Garage and Keycloak on 127.0.0.1, then 28 checks that each one works' },
		{ command: 'scripts/migrate.sh', note: 'apply the migrations as the owner login' },
		{ command: 'set -a; . deploy/local.env; set +a', note: 'the local development settings' },
		{ command: 'cargo run --release -p colophon-server', note: 'the API and the worker, in one process' },
		{ command: 'cargo run --release -p colophon-cli -- doctor', note: 'exits 0 only when every dependency is ready' }
	];
</script>

<Seo
	title="Open source and self-hosting · Colophon"
	description="Colophon's code, plan, decisions and load results are public. How to run it yourself, what it is built with, and the licence's status: Apache-2.0 proposed, not yet confirmed."
/>

<PageIntro badge="Open source" title="Public code, public plan, public evidence">
	<p>
		The code, the plan, every decision and every load result, failures included, live in one repository. Colophon is built in the open by one
		maintainer pairing with Claude Code, one tested step at a time, and every step is written up in the
		<a class="text-link" href={BUILD_LOG}>build log</a>.
	</p>
	{#snippet aside()}
		<Pill label="The repository" href={GITHUB}>
			{#snippet icon()}<ArrowUpRightIcon class="size-4" aria-hidden="true" />{/snippet}
		</Pill>
	{/snippet}
</PageIntro>

<div class="shell mt-14 grid gap-20">
	<section aria-labelledby="licence" class="grid gap-8 lg:grid-cols-[minmax(0,22rem)_minmax(0,1fr)]">
		<SectionHeading id="licence" eyebrow="Licence" title="Proposed, not yet confirmed" />
		<div class="glass rounded-3xl p-6 sm:p-8">
			<Badge variant="pending">Open decision O-014</Badge>
			<p class="mt-4 text-[1.0625rem] leading-relaxed">
				Apache-2.0 is proposed for the core, the command line and the SDK. It is <strong class="font-semibold">not yet confirmed</strong>: the maintainer
				approves it before the first public release, and the <code class="font-mono text-[0.9em]">LICENSE</code> file is added then.
			</p>
			<p class="mt-3 text-[0.9375rem] leading-relaxed text-muted-foreground">
				Until then, the repository has no licence file. Dependencies are reviewed separately and the bundle is never described as uniformly permissive:
				Garage, the local object store, is AGPL-3.0, which doesn’t reach Colophon because it neither ships nor modifies it (D-016).
			</p>
		</div>
	</section>

	<section aria-labelledby="run" class="grid gap-8 lg:grid-cols-[minmax(0,22rem)_minmax(0,1fr)]">
		<SectionHeading id="run" eyebrow="Self-hosting" title="Run it on your own machine">
			Pre-release means local only: nothing is packaged for production yet, and a guided install from the README by someone other than the
			maintainer is a v0.1 gate.
			<span class="mt-5 block"><Pill label="The quickstart in the docs" href={QUICKSTART} external variant="outline" /></span>
		</SectionHeading>
		<div class="space-y-4">
			<div class="glass rounded-3xl p-6">
				<h3 class="text-sm font-semibold">You need</h3>
				<p class="mt-2 text-[0.9375rem] leading-relaxed text-muted-foreground">
					rustup (it installs the pinned Rust 1.99.1), Docker with Compose, and Node 22.17 or later; optionally cargo-deny and k6.
				</p>
			</div>
			<ol class="overflow-hidden glass rounded-3xl">
				{#each LOCAL as step, index (step.command)}
					<li class="grid gap-1 border-b border-border px-6 py-4 last:border-b-0 sm:grid-cols-[2rem_minmax(0,1fr)]">
						<span class="figure text-sm text-muted-foreground">{index + 1}</span>
						<div class="min-w-0">
							<code class="block overflow-x-auto font-mono text-[0.8125rem] whitespace-nowrap">{step.command}</code>
							<p class="mt-1 text-sm text-muted-foreground">{step.note}</p>
						</div>
					</li>
				{/each}
			</ol>
			<p class="text-sm text-muted-foreground">
				Then sign in at <code class="font-mono">localhost:58000</code> with the development realm’s public test account. Every port binds to
				127.0.0.1, and its credentials are public development values: never reuse them. Details:
				<a class="text-link" href={GITHUB_FILE('deploy/README.md')}>deploy/README.md</a>.
			</p>
		</div>
	</section>

	<section aria-labelledby="stack" class="grid gap-8 lg:grid-cols-[minmax(0,22rem)_minmax(0,1fr)]">
		<SectionHeading id="stack" eyebrow="Stack" title="What it is built with">
			Exact versions are pinned: Rust in rust-toolchain.toml, the JavaScript workspace in its lockfile, the local stack’s images by digest.
		</SectionHeading>
		<dl class="overflow-hidden glass rounded-3xl">
			{#each STACK as row (row.layer)}
				<div class="grid gap-1 border-b border-border px-6 py-4 last:border-b-0 sm:grid-cols-[9rem_minmax(0,1fr)] sm:gap-4">
					<dt class="text-sm font-semibold">{row.layer}</dt>
					<dd class="text-[0.9375rem] leading-relaxed text-muted-foreground">{row.choice}</dd>
				</div>
			{/each}
		</dl>
	</section>

	<section aria-labelledby="contribute" class="grid gap-8 lg:grid-cols-[minmax(0,22rem)_minmax(0,1fr)]">
		<SectionHeading id="contribute" eyebrow="Contributing" title="How to help today" />
		<div class="grid gap-4 sm:grid-cols-2">
			<div class="glass rounded-3xl p-6">
				<h3 class="text-[0.9375rem] font-semibold">Design feedback, welcome now</h3>
				<p class="mt-2 text-sm leading-relaxed text-muted-foreground">
					Open an issue: a failure mode the plan misses, a guarantee stronger than it can be, or a simpler way to prove something.
				</p>
			</div>
			<div class="glass rounded-3xl p-6">
				<h3 class="text-[0.9375rem] font-semibold">Code, once the rules are written</h3>
				<p class="mt-2 text-sm leading-relaxed text-muted-foreground">
					Code contributions open when a CONTRIBUTING file exists, alongside a security policy and the other project files planned for v0.1.
				</p>
			</div>
			<div class="glass rounded-3xl p-6 sm:col-span-2">
				<h3 class="text-[0.9375rem] font-semibold">How changes are made</h3>
				<p class="mt-2 text-sm leading-relaxed text-muted-foreground">
					Work happens on a branch per step and merges into main when its proof passes: regression tests, a page in the separate test app, and a
					load scenario with committed results. Database migrations, authentication, publishing transitions and provider adapters get a
					line-by-line human review. <a class="text-link" href={GITHUB_FILE('AGENTS.md')}>AGENTS.md</a> holds the rules AI coding agents follow
					here.
				</p>
			</div>
		</div>
	</section>
</div>
