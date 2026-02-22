<script lang="ts">
	import { page } from '$app/stores';
	import { api, ApiClientError } from '$lib/api/client';
	import type { DeviceInfo, TelemetryResponse } from '$lib/types';
	import StatusBadge from '$lib/components/StatusBadge.svelte';
	import CommandForm from '$lib/components/CommandForm.svelte';

	let device = $state<DeviceInfo | null>(null);
	let telemetry = $state<TelemetryResponse | null>(null);
	let loading = $state(true);
	let error = $state<string | null>(null);

	const deviceId = $derived($page.params.id ?? '');

	async function loadDevice() {
		if (!deviceId) return;
		loading = true;
		error = null;
		try {
			const [d, t] = await Promise.all([
				api.getDevice(deviceId),
				api.getTelemetry(deviceId)
			]);
			device = d;
			telemetry = t;
		} catch (err) {
			error = err instanceof ApiClientError ? err.message : 'Failed to load device';
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		if (deviceId) loadDevice();
	});
</script>

<div class="space-y-8">
	<div>
		<a href="/" class="text-sm text-text-muted hover:text-text">&larr; Back to devices</a>
	</div>

	{#if loading}
		<p class="text-text-muted">Loading device...</p>
	{:else if error}
		<div class="rounded-md border border-danger/20 bg-danger/5 p-4 text-sm text-danger">
			{error}
		</div>
	{:else if device}
		<div class="space-y-6">
			<div class="flex items-start justify-between">
				<div>
					<h1 class="font-mono text-2xl font-bold">{device.device_id}</h1>
					<p class="mt-1 text-text-muted">Fleet: {device.fleet_id}</p>
				</div>
				<StatusBadge status={device.status} />
			</div>

			<div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
				<div class="rounded-lg border border-border bg-white p-4">
					<dt class="text-xs font-medium text-text-muted uppercase">Hardware</dt>
					<dd class="mt-1 font-mono text-sm">{device.hardware_type}</dd>
				</div>
				<div class="rounded-lg border border-border bg-white p-4">
					<dt class="text-xs font-medium text-text-muted uppercase">VIN</dt>
					<dd class="mt-1 font-mono text-sm">{device.vin ?? 'Not available'}</dd>
				</div>
				<div class="rounded-lg border border-border bg-white p-4">
					<dt class="text-xs font-medium text-text-muted uppercase">Last Heartbeat</dt>
					<dd class="mt-1 text-sm">
						{device.last_heartbeat
							? new Date(device.last_heartbeat).toLocaleString()
							: 'Never'}
					</dd>
				</div>
				<div class="rounded-lg border border-border bg-white p-4">
					<dt class="text-xs font-medium text-text-muted uppercase">Certificate</dt>
					<dd class="mt-1 font-mono text-sm">{device.certificate_id ?? 'None'}</dd>
				</div>
				<div class="rounded-lg border border-border bg-white p-4">
					<dt class="text-xs font-medium text-text-muted uppercase">Created</dt>
					<dd class="mt-1 text-sm">{new Date(device.created_at).toLocaleDateString()}</dd>
				</div>
			</div>

			<section class="rounded-lg border border-border bg-white p-6">
				<h2 class="mb-4 text-lg font-semibold">Command Interface</h2>
				<CommandForm
					deviceId={device.device_id}
					fleetId={typeof device.fleet_id === 'string' ? device.fleet_id : ''}
				/>
			</section>

			{#if telemetry}
				<section class="rounded-lg border border-border bg-white p-6">
					<h2 class="mb-4 text-lg font-semibold">Telemetry</h2>
					{#if telemetry.readings.length === 0}
						<p class="text-sm text-text-muted">
							{telemetry.message ?? 'No telemetry data available.'}
						</p>
					{:else}
						<pre class="rounded bg-surface p-4 text-xs">{JSON.stringify(telemetry.readings, null, 2)}</pre>
					{/if}
				</section>
			{/if}
		</div>
	{/if}
</div>
