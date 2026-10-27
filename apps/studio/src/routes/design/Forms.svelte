<script lang="ts">
	import EyeIcon from '@lucide/svelte/icons/eye';
	import LockIcon from '@lucide/svelte/icons/lock';
	import SearchIcon from '@lucide/svelte/icons/search';
	import ShieldCheckIcon from '@lucide/svelte/icons/shield-check';
	import { Button } from '@colophon/ui/ui/button';
	import { Checkbox } from '@colophon/ui/ui/checkbox';
	import * as Field from '@colophon/ui/ui/field';
	import { Input } from '@colophon/ui/ui/input';
	import * as InputGroup from '@colophon/ui/ui/input-group';
	import * as InputOTP from '@colophon/ui/ui/input-otp';
	import { Label } from '@colophon/ui/ui/label';
	import * as NativeSelect from '@colophon/ui/ui/native-select';
	import * as RadioGroup from '@colophon/ui/ui/radio-group';
	import * as Select from '@colophon/ui/ui/select';
	import { Slider } from '@colophon/ui/ui/slider';
	import { Switch } from '@colophon/ui/ui/switch';
	import { Textarea } from '@colophon/ui/ui/textarea';
	import Mark from '#lib/shell/Mark.svelte';
	import Specimen from './Specimen.svelte';

	const ROLES = [
		{ value: 'owner', label: 'Owner' },
		{ value: 'developer', label: 'Developer' },
		{ value: 'editor', label: 'Editor' },
		{ value: 'publisher', label: 'Publisher' }
	];
	let role = $state('editor');
	const roleLabel = $derived(ROLES.find((option) => option.value === role)?.label ?? 'Choose a role');
	let visibility = $state('public');
	let autosave = $state(true);
	let width = $state(68);
	let password = $state('');
	const strength = $derived(Math.min(4, Math.floor(password.length / 3)));
</script>

<div class="grid gap-14 lg:grid-cols-[minmax(0,26rem)_minmax(0,1fr)]">
	<div class="haze rounded-[2rem] border border-border p-8 shadow-hairline">
		<div class="mb-10 flex items-center gap-2.5 font-semibold"><Mark size={24} />Colophon</div>
		<h3 class="display-2">Sign in to the Studio</h3>
		<p class="mt-2 text-[0.9375rem] text-muted-foreground">Write, review and publish, with proof it reached the site.</p>
		<Button variant="outline" size="lg" class="mt-8 w-full">Continue with Keycloak</Button>
		<div class="my-6 flex items-center gap-3 text-xs text-muted-foreground">
			<span class="h-px flex-1 bg-border"></span>or with email<span class="h-px flex-1 bg-border"></span>
		</div>
		<form class="grid gap-5" onsubmit={(event) => event.preventDefault()}>
			<div class="grid grid-cols-2 gap-3">
				<div class="grid gap-2"><Label for="f-name">Full name</Label><Input id="f-name" class="h-12" placeholder="Ada Lovelace" /></div>
				<div class="grid gap-2"><Label for="f-project">Project</Label><Input id="f-project" class="h-12" placeholder="my-site" /></div>
			</div>
			<div class="grid gap-2"><Label for="f-email">Email</Label><Input id="f-email" type="email" class="h-12" placeholder="you@example.com" /></div>
			<div class="grid gap-2">
				<Label for="f-password">Password</Label>
				<InputGroup.Root class="h-12">
					<InputGroup.Input id="f-password" type="password" placeholder="At least 12 characters" bind:value={password} />
					<InputGroup.Addon align="inline-end"><EyeIcon class="size-4" /></InputGroup.Addon>
				</InputGroup.Root>
				<div class="grid grid-cols-4 gap-1.5" aria-hidden="true">
					{#each [1, 2, 3, 4] as bar (bar)}
						<span class="h-1 rounded-full {bar <= strength ? 'bg-foreground' : 'bg-border'}"></span>
					{/each}
				</div>
			</div>
			<Button size="lg" type="submit" class="mt-2 w-full">Sign in</Button>
			<p class="flex flex-wrap justify-center gap-x-4 gap-y-1 text-xs text-muted-foreground">
				<span class="flex items-center gap-1"><ShieldCheckIcon class="size-3.5 text-live" />Server-side sessions</span>
				<span class="flex items-center gap-1"><LockIcon class="size-3.5 text-live" />CSRF and Origin checks</span>
			</p>
		</form>
	</div>

	<div class="grid content-start gap-10">
		<div class="grid gap-8 sm:grid-cols-2">
			<Specimen label="Text, with a hint">
				<Field.Field>
					<Field.Label for="s-title">Title</Field.Label>
					<Input id="s-title" value="Hello, world" />
					<Field.Description>Up to 200 characters.</Field.Description>
				</Field.Field>
			</Specimen>
			<Specimen label="Refused, said in words">
				<Field.Field data-invalid="true">
					<Field.Label for="s-slug">Slug</Field.Label>
					<Input id="s-slug" class="font-mono" value="Hello World" aria-invalid="true" />
					<Field.Error>Lowercase letters, digits and single hyphens, like hello-world.</Field.Error>
				</Field.Field>
			</Specimen>
			<Specimen label="Search">
				<InputGroup.Root>
					<InputGroup.Addon><SearchIcon class="size-4" /></InputGroup.Addon>
					<InputGroup.Input placeholder="Search titles" />
				</InputGroup.Root>
			</Specimen>
			<Specimen label="Disabled">
				<Input disabled value="Published entries can't be edited here" />
			</Specimen>
			<Specimen label="Several lines" class="sm:col-span-2">
				<Textarea placeholder="A short summary for lists and feeds." />
			</Specimen>
		</div>

		<div class="grid gap-8 sm:grid-cols-2">
			<Specimen label="Select">
				<Select.Root type="single" bind:value={role}>
					<Select.Trigger class="w-full">{roleLabel}</Select.Trigger>
					<Select.Content>
						{#each ROLES as option (option.value)}
							<Select.Item value={option.value} label={option.label}>{option.label}</Select.Item>
						{/each}
					</Select.Content>
				</Select.Root>
			</Specimen>
			<Specimen label="Native select">
				<NativeSelect.Root class="w-full">
					<NativeSelect.Option value="news">News</NativeSelect.Option>
					<NativeSelect.Option value="guides">Guides</NativeSelect.Option>
				</NativeSelect.Root>
			</Specimen>
			<Specimen label="Checkbox and switch">
				<div class="grid gap-4">
					<div class="flex items-center gap-3"><Checkbox id="c-featured" checked /><Label for="c-featured">Featured</Label></div>
					<div class="flex items-center gap-3"><Switch id="c-autosave" bind:checked={autosave} /><Label for="c-autosave">Autosave drafts</Label></div>
				</div>
			</Specimen>
			<Specimen label="Radio group">
				<RadioGroup.Root bind:value={visibility}>
					<div class="flex items-center gap-3"><RadioGroup.Item value="public" id="r-public" /><Label for="r-public">Public</Label></div>
					<div class="flex items-center gap-3"><RadioGroup.Item value="private" id="r-private" /><Label for="r-private">Private</Label></div>
				</RadioGroup.Root>
			</Specimen>
			<Specimen label="Slider: measure {width} characters">
				<Slider type="single" bind:value={width} min={45} max={90} step={1} aria-label="Measure" />
			</Specimen>
			<Specimen label="One-time code">
				<InputOTP.Root maxlength={6}>
					{#snippet children({ cells })}
						<InputOTP.Group>
							{#each cells.slice(0, 3) as cell, index (index)}<InputOTP.Slot {cell} />{/each}
						</InputOTP.Group>
						<InputOTP.Separator />
						<InputOTP.Group>
							{#each cells.slice(3, 6) as cell, index (index)}<InputOTP.Slot {cell} />{/each}
						</InputOTP.Group>
					{/snippet}
				</InputOTP.Root>
			</Specimen>
		</div>
	</div>
</div>
