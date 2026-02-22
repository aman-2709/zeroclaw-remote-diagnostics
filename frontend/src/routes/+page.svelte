<script lang="ts">
	import { api, ApiClientError } from '$lib/api/client';
	import type { DeviceSummary, WsEvent } from '$lib/types';
	import DeviceCard from '$lib/components/DeviceCard.svelte';
	import { wsStore } from '$lib/stores/websocket.svelte';
	import { onMount } from 'svelte';

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

	onMount(() => {
		loadDevices();

		const unsub = wsStore.onEvent((event: WsEvent) => {
			if (event.type === 'device_heartbeat') {
				// Update the heartbeat timestamp for this device
				devices = devices.map((d) =>
					d.device_id === event.device_id
						? { ...d, last_heartbeat: event.timestamp, status: 'online' as const }
						: d
				);
			} else if (event.type === 'device_status_changed') {
				devices = devices.map((d) =>
					d.device_id === event.device_id
						? { ...d, status: event.new_status as DeviceSummary['status'] }
						: d
				);
			}
		});

		return unsub;
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
