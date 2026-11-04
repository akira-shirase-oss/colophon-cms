<script lang="ts">
	import { onMount } from 'svelte';
	import { resolve } from '$app/paths';
	import { call, csrfToken, detailsOf, envelopeOf, listProjects, type Reply } from '#lib/api.ts';

	interface Project {
		id: string;
		slug: string;
		name: string;
	}
	interface Counts {
		expected: number;
		done: number;
		blocked: number;
	}
	interface Plan {
		id: string;
		collection: string;
		base_version: number;
		target_version: number | null;
		state: string;
		hash: string;
		items: Counts;
		republish_required?: boolean;
		error: string | null;
	}
	interface Item {
		entry_id: string;
		title: string | null;
		outcome: string;
		problem: { reason: string; pointer: string; message: string } | null;
		new_revision_id: string | null;
	}

	let projects = $state<Project[]>([]);
	let signedIn = $state<boolean | null>(null);
	let projectId = $state('');
	let plans = $state<Plan[]>([]);
	let plan = $state<Plan | null>(null);
	let items = $state<Item[]>([]);
	let outcome = $state('');
	let collection = $state('articles');
	let definition = $state('');
	let resolution = $state('');
	let resolving = $state('');
	let message = $state('');

	const base = $derived(`/api/v1/projects/${projectId}/environments/production/migrations`);

	function headers(extra: Record<string, string> = {}): Record<string, string> {
		return { 'Content-Type': 'application/json', 'X-CSRF-Token': csrfToken(), 'Idempotency-Key': crypto.randomUUID(), ...extra };
	}

	function describe(reply: Reply): string {
		const error = envelopeOf(reply.body);
		const details = detailsOf(reply.body);
		return `${reply.status} ${error?.code ?? ''}${details ? ` (${details.reason}${details.pointer ? ` at ${details.pointer}` : ''})` : ''}`;
	}

	async function loadPlans() {
		const reply = await call('GET', `${base}?limit=100`);
		plans = reply.status === 200 ? (reply.body as { data: Plan[] }).data : [];
		if (plan !== null) await open(plan.id);
	}

	async function loadItems() {
		if (plan === null) return;
		const query = outcome === '' ? '' : `?outcome=${outcome}`;
		const reply = await call('GET', `${base}/${plan.id}/items${query}${query === '' ? '?' : '&'}limit=100`);
		items = reply.status === 200 ? (reply.body as { data: Item[] }).data : [];
	}

	async function open(id: string) {
		const reply = await call('GET', `${base}/${id}`);
		if (reply.status !== 200) {
			message = describe(reply);
			return;
		}
		plan = (reply.body as { data: Plan }).data;
		await loadItems();
	}

	async function create(event: SubmitEvent) {
		event.preventDefault();
		let parsed: unknown;
		try {
			parsed = JSON.parse(definition);
		} catch {
			message = 'the definition is not JSON';
			return;
		}
		const reply = await call('POST', base, { headers: headers(), body: JSON.stringify({ collection, definition: parsed }) });
		if (reply.status === 201) {
			const made = (reply.body as { data: Plan }).data;
			message = `planned ${made.id} on ${made.collection}: ${made.items.blocked} of ${made.items.expected} in the way`;
			await loadPlans();
			await open(made.id);
		} else {
			message = describe(reply);
		}
	}

	/** A plan action; hash-bound ones name the plan on screen (D-100). */
	async function act(action: 'approve' | 'apply' | 'cancel') {
		if (plan === null) return;
		const bound: Record<string, string> = action === 'cancel' ? {} : { 'If-Match': `"${plan.hash}"` };
		const reply = await call('POST', `${base}/${plan.id}/${action}`, { headers: headers(bound) });
		message = reply.status === 200 || reply.status === 202 ? `${action}: ${(reply.body as { data: { state: string } }).data.state}` : describe(reply);
		await loadPlans();
	}

	async function resolveItem(event: SubmitEvent) {
		event.preventDefault();
		if (plan === null) return;
		let parsed: unknown;
		try {
			parsed = JSON.parse(resolution);
		} catch {
			message = 'the resolution is not JSON';
			return;
		}
		const reply = await call('PUT', `${base}/${plan.id}/items/${resolving}`, {
			headers: headers({ 'If-Match': `"${plan.hash}"` }),
			body: JSON.stringify({ resolution: parsed })
		});
		message = reply.status === 200 ? `resolved ${resolving}` : describe(reply);
		resolving = '';
		await loadPlans();
	}

	/** Polls the open plan until it leaves `running` (the worker drives it, D-101). */
	async function follow() {
		if (plan === null) return;
		const reply = await call('GET', `${base}/${plan.id}`);
		if (reply.status === 200) {
			plan = (reply.body as { data: Plan }).data;
			await loadItems();
		}
	}

	onMount(async () => {
		const reply = await listProjects<Project>();
		signedIn = reply.status === 200;
		projects = reply.projects;
		if (projectId === '' && projects.length > 0) projectId = projects[0]?.id ?? '';
		if (projectId !== '') await loadPlans();
	});
</script>

<h1>Migrations</h1>
<p class="lede">
	Content-schema migrations through the real API (D-100, D-101): plan a change stored entries can't take as they are, resolve the items
	the dry run names, approve on the plan's hash, apply, and watch the worker drive the run.
</p>

{#if signedIn === false}
	<p data-testid="signed-out">Not signed in. <a href={resolve('/session')}>Sign in on the Session page</a> first.</p>
{:else if signedIn}
	<label class="field">
		Project
		<select bind:value={projectId} onchange={loadPlans} name="project">
			{#each projects as project (project.id)}
				<option value={project.id}>{project.name} ({project.slug})</option>
			{/each}
		</select>
	</label>

	<ul data-testid="plans">
		{#each plans as one (one.id)}
			<li data-id={one.id} data-state={one.state}>
				<button class="link" onclick={() => open(one.id)}>{one.collection} v{one.base_version}</button>
				{one.state}, {one.items.done}/{one.items.expected} done{#if one.items.blocked > 0}, {one.items.blocked} in the way{/if}
			</li>
		{/each}
	</ul>

	{#if message}<p data-testid="message">{message}</p>{/if}

	<section>
		<h2>New plan</h2>
		<form onsubmit={create}>
			<label class="field">Collection <input bind:value={collection} name="collection" /></label>
			<label class="field">Target definition <textarea bind:value={definition} name="definition" rows="8"></textarea></label>
			<button type="submit">Plan migration</button>
		</form>
	</section>

	{#if plan !== null}
		<section data-testid="plan" data-state={plan.state} data-hash={plan.hash}>
			<h2>Plan on {plan.collection} <small data-testid="plan-state">{plan.state}</small></h2>
			<p>
				v{plan.base_version} → {plan.target_version === null ? 'not stamped yet' : `v${plan.target_version}`}; {plan.items.done}/{plan.items.expected}
				done{#if plan.items.blocked > 0}, {plan.items.blocked} in the way{/if}{#if plan.republish_required}; republish required{/if}{#if plan.error}; error: {plan.error}{/if}
			</p>
			<div class="row">
				<button onclick={() => act('approve')} disabled={plan.state !== 'proposed'}>Approve</button>
				<button onclick={() => act('apply')} disabled={plan.state !== 'approved'}>Apply</button>
				<button onclick={() => act('cancel')} disabled={plan.state === 'running' || plan.state === 'done'}>Cancel</button>
				<button onclick={follow}>Refresh</button>
			</div>

			<label class="field">
				Outcome
				<select bind:value={outcome} onchange={loadItems} name="outcome">
					<option value="">all</option>
					{#each ['ok', 'migrates', 'needs_input', 'resolved', 'applied', 'skipped', 'blocked'] as one (one)}
						<option value={one}>{one}</option>
					{/each}
				</select>
			</label>
			<ul data-testid="items">
				{#each items as item (item.entry_id)}
					<li data-entry={item.entry_id} data-outcome={item.outcome}>
						{item.title ?? item.entry_id} — {item.outcome}{#if item.problem} ({item.problem.reason}: {item.problem.message}){/if}
						{#if item.outcome === 'needs_input'}<button class="link" onclick={() => (resolving = item.entry_id)}>resolve</button>{/if}
					</li>
				{/each}
			</ul>

			{#if resolving !== ''}
				<form onsubmit={resolveItem} data-testid="resolving" data-entry={resolving}>
					<label class="field">Resolution for {resolving} <textarea bind:value={resolution} name="resolution" rows="8"></textarea></label>
					<button type="submit">Save resolution</button>
				</form>
			{/if}
		</section>
	{/if}
{/if}
