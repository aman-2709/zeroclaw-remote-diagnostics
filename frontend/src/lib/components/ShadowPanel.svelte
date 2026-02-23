<script lang="ts">
	import { api } from '$lib/api/client';
	import type { ShadowSummary, ShadowResponse, WsEvent } from '$lib/types';
	import { timeAgo } from '$lib/utils/format';
	import { wsStore } from '$lib/stores/websocket.svelte';
	import { onMount } from 'svelte';
	import JsonView from './JsonView.svelte';

	let { deviceId }: { deviceId: string } = $props();

	let shadows = $state<ShadowSummary[]>([]);
	let selected = $state<ShadowResponse | null>(null);
	let loading = $state(true);
	let detailLoading = $state(false);
	let error = $state<string | null>(null);

	// Edit desired state
	let editing = $state(false);
	let editJson = $state('');
	let editError = $state<string | null>(null);
	let saving = $state(false);

	const deltaKeys = $derived(
		selected?.delta && typeof selected.delta === 'object'
			? Object.keys(selected.delta as Record<string, unknown>)
			: [] as string[]
	);

	async function loadShadows() {
		loading = true;
		error = null;
		try {
			shadows = await api.listShadows(deviceId);
		} catch {
			error = 'Failed to load shadows';
		} finally {
			loading = false;
		}
	}

	async function selectShadow(name: string) {
		detailLoading = true;
		editing = false;
		try {
			selected = await api.getShadow(deviceId, name);
		} catch {
			error = `Failed to load shadow "${name}"`;
		} finally {
			detailLoading = false;
		}
	}

	function startEdit() {
		editing = true;
		editError = null;
		editJson = JSON.stringify(selected?.desired ?? {}, null, 2);
	}

	function cancelEdit() {
		editing = false;
		editError = null;
	}

	async function saveDesired() {
		if (!selected) return;
		saving = true;
		editError = null;
		try {
			const parsed = JSON.parse(editJson);
			selected = await api.setDesired(deviceId, selected.shadow_name, parsed);
			editing = false;
			// Refresh list to update version/timestamp
			await loadShadows();
		} catch (err) {
			editError = err instanceof SyntaxError ? 'Invalid JSON' : 'Failed to save';
		} finally {
			saving = false;
		}
	}

	onMount(() => {
		loadShadows();

		const unsub = wsStore.onEvent((event: WsEvent) => {
			if (event.type === 'shadow_updated' && event.device_id === deviceId) {
				loadShadows();
				if (selected && selected.shadow_name === event.shadow_name) {
					selectShadow(event.shadow_name);
				}
			}
		});

		return unsub;
	});
</script>

<div class="space-y-4">
	{#if loading}
		<p class="text-sm text-text-muted">Loading shadows...</p>
	{:else if error && shadows.length === 0}
		<p class="text-sm text-danger">{error}</p>
	{:else if shadows.length === 0}
		<p class="text-sm text-text-muted">No device shadows available.</p>
	{:else}
		<!-- Shadow list -->
		<div class="flex flex-wrap gap-2">
			{#each shadows as shadow}
				<button
					onclick={() => selectShadow(shadow.shadow_name)}
					class="rounded-md border px-3 py-1.5 text-sm transition {selected?.shadow_name === shadow.shadow_name ? 'border-primary bg-primary/5 font-medium text-primary' : 'border-border bg-white text-text hover:border-primary/50'}"
				>
					{shadow.shadow_name}
					<span class="ml-1 text-xs text-text-muted">v{shadow.version}</span>
				</button>
			{/each}
		</div>

		<!-- Shadow detail -->
		{#if detailLoading}
			<p class="text-sm text-text-muted">Loading shadow detail...</p>
		{:else if selected}
			<div class="rounded-lg border border-border bg-white p-4">
				<div class="mb-3 flex items-center justify-between">
					<div>
						<h3 class="font-mono text-sm font-semibold">{selected.shadow_name}</h3>
						<p class="text-xs text-text-muted">
							v{selected.version} &middot; updated {timeAgo(selected.last_updated)}
						</p>
					</div>
					{#if !editing}
						<button
							onclick={startEdit}
							class="rounded-md border border-border px-3 py-1 text-xs text-text-muted transition hover:border-primary hover:text-primary"
						>
							Edit Desired
						</button>
					{/if}
				</div>

				{#if editing}
					<div class="space-y-2">
						<label for="desired-json-editor" class="block text-xs font-medium text-text-muted">Desired State (JSON)</label>
						<textarea
							id="desired-json-editor"
							bind:value={editJson}
							rows={8}
							class="w-full rounded-md border border-border bg-surface p-3 font-mono text-sm outline-none focus:border-primary focus:ring-1 focus:ring-primary"
						></textarea>
						{#if editError}
							<p class="text-xs text-danger">{editError}</p>
						{/if}
						<div class="flex gap-2">
							<button
								onclick={saveDesired}
								disabled={saving}
								class="rounded-md bg-primary px-3 py-1 text-xs font-medium text-white hover:bg-primary-dark disabled:opacity-50"
							>
								{saving ? 'Saving...' : 'Save'}
							</button>
							<button
								onclick={cancelEdit}
								class="rounded-md border border-border px-3 py-1 text-xs text-text-muted hover:text-text"
							>
								Cancel
							</button>
						</div>
					</div>
				{:else}
					<div class="grid gap-4 sm:grid-cols-2">
						<div>
							<h4 class="mb-2 text-xs font-medium uppercase text-text-muted">Reported</h4>
							<div class="rounded-md bg-surface p-3">
								{#if selected.reported}
									<JsonView data={selected.reported} highlightKeys={deltaKeys} />
								{:else}
									<span class="text-xs text-text-muted">No reported state</span>
								{/if}
							</div>
						</div>
						<div>
							<h4 class="mb-2 text-xs font-medium uppercase text-text-muted">Desired</h4>
							<div class="rounded-md bg-surface p-3">
								{#if selected.desired}
									<JsonView data={selected.desired} highlightKeys={deltaKeys} />
								{:else}
									<span class="text-xs text-text-muted">No desired state</span>
								{/if}
							</div>
						</div>
					</div>
					{#if deltaKeys.length > 0}
						<div class="mt-3 rounded-md border border-warning/30 bg-warning/5 p-2">
							<p class="text-xs font-medium text-warning">
								Delta: {deltaKeys.join(', ')}
							</p>
						</div>
					{/if}
				{/if}
			</div>
		{/if}
	{/if}
</div>
