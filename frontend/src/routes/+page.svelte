<script lang="ts">
	import { api, ApiClientError } from '$lib/api/client';
	import type { DeviceSummary } from '$lib/types';
	import DeviceCard from '$lib/components/DeviceCard.svelte';

	let devices = $state<DeviceSummary[]>([]);
	let loading = $state(true);
	let error = $state<string | null>(null);

	async function loadDevices() {
		loading = true;
		error = null;
		try {
			devices = await api.listDevices();
		} catch (err) {
			error = err instanceof ApiClientError ? err.message : 'Failed to load devices';
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		loadDevices();
	});
</script>

<div class="space-y-6">
	<div class="flex items-center justify-between">
		<h1 class="text-2xl font-bold">Devices</h1>
		<button
			onclick={loadDevices}
			disabled={loading}
			class="rounded-md border border-border px-3 py-1.5 text-sm hover:bg-white disabled:opacity-50"
		>
			Refresh
		</button>
	</div>

	{#if loading}
		<p class="text-text-muted">Loading devices...</p>
	{:else if error}
		<div class="rounded-md border border-danger/20 bg-danger/5 p-4 text-sm text-danger">
			{error}
		</div>
	{:else if devices.length === 0}
		<p class="text-text-muted">No devices registered.</p>
	{:else}
		<div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
			{#each devices as device (device.device_id)}
				<DeviceCard {device} />
			{/each}
		</div>
	{/if}
</div>
