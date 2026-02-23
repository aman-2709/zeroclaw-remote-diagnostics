<script lang="ts">
	import { page } from '$app/stores';
	import { api, ApiClientError } from '$lib/api/client';
	import type { DeviceInfo, CommandSummary, ShadowResponse, WsEvent } from '$lib/types';
	import { formatHardwareType } from '$lib/types/device';
	import { timeAgo, formatUptime } from '$lib/utils/format';
	import StatusBadge from '$lib/components/StatusBadge.svelte';
	import CommandForm from '$lib/components/CommandForm.svelte';
	import ServiceIndicator from '$lib/components/ServiceIndicator.svelte';
	import ShadowPanel from '$lib/components/ShadowPanel.svelte';
	import TelemetryPanel from '$lib/components/TelemetryPanel.svelte';
	import { wsStore } from '$lib/stores/websocket.svelte';
	import { onMount } from 'svelte';

	let device = $state<DeviceInfo | null>(null);
	let loading = $state(true);
	let error = $state<string | null>(null);

	// Tab state
	let activeTab = $state<'overview' | 'commands' | 'shadows' | 'telemetry'>('overview');

	// Commands tab state
	let commands = $state<CommandSummary[]>([]);
	let commandsLoading = $state(false);

	// Overview service status (from "state" shadow)
	let stateShadow = $state<ShadowResponse | null>(null);

	// Heartbeat pulse
	let lastHeartbeat = $state<string | null>(null);
	const heartbeatRecent = $derived(
		lastHeartbeat ? Date.now() - new Date(lastHeartbeat).getTime() < 60000 : false
	);

	const deviceId = $derived($page.params.id ?? '');

	async function loadDevice() {
		if (!deviceId) return;
		loading = true;
		error = null;
		try {
			device = await api.getDevice(deviceId);
			lastHeartbeat = device.last_heartbeat;

			// Load "state" shadow for service status (best effort)
			try {
				stateShadow = await api.getShadow(deviceId, 'state');
			} catch {
				stateShadow = null;
			}
		} catch (err) {
			error = err instanceof ApiClientError ? err.message : 'Failed to load device';
		} finally {
			loading = false;
		}
	}

	async function loadDeviceCommands() {
		commandsLoading = true;
		try {
			const all = await api.listCommands();
			commands = all.filter((c) => c.device_id === deviceId);
		} catch {
			// Non-critical
		} finally {
			commandsLoading = false;
		}
	}

	// Service status derived from shadow reported state
	const reported = $derived(
		stateShadow?.reported && typeof stateShadow.reported === 'object'
			? (stateShadow.reported as Record<string, unknown>)
			: null
	);

	const ollamaStatus = $derived(
		reported?.ollama_status === 'connected' ? 'running' :
		reported?.ollama_status === 'disabled' ? 'stopped' :
		reported?.ollama_status ? String(reported.ollama_status) : undefined
	);

	const canStatus = $derived(
		reported?.can_interface === 'mock' ? 'stopped' :
		reported?.can_interface ? 'running' : undefined
	);

	const uptimeValue = $derived(
		typeof reported?.uptime_secs === 'number' ? formatUptime(reported.uptime_secs) : undefined
	);

	const agentVersion = $derived(
		typeof reported?.agent_version === 'string' ? reported.agent_version : undefined
	);

	$effect(() => {
		if (deviceId) loadDevice();
	});

	$effect(() => {
		if (activeTab === 'commands' && deviceId && commands.length === 0) {
			loadDeviceCommands();
		}
	});

	onMount(() => {
		const unsub = wsStore.onEvent((event: WsEvent) => {
			if (event.type === 'device_heartbeat' && event.device_id === deviceId) {
				lastHeartbeat = event.timestamp;
				if (device) {
					device = { ...device, last_heartbeat: event.timestamp };
				}
			} else if (event.type === 'device_status_changed' && event.device_id === deviceId) {
				if (device) {
					device = { ...device, status: event.new_status as DeviceInfo['status'] };
				}
			} else if (event.type === 'command_dispatched' && event.device_id === deviceId) {
				const summary: CommandSummary = {
					id: event.command_id,
					device_id: event.device_id,
					command: event.command,
					status: 'pending',
					created_at: event.created_at
				};
				commands = [summary, ...commands];
			} else if (event.type === 'command_response' && event.device_id === deviceId) {
				commands = commands.map((cmd) =>
					cmd.id === event.command_id
						? {
								...cmd,
								status: event.status as CommandSummary['status'],
								response_text: event.response_text
							}
						: cmd
				);
			} else if (event.type === 'shadow_updated' && event.device_id === deviceId && event.shadow_name === 'state') {
				// Refresh service status
				api.getShadow(deviceId, 'state').then((s) => (stateShadow = s)).catch(() => {});
			}
		});

		return unsub;
	});

	const TABS: { key: typeof activeTab; label: string }[] = [
		{ key: 'overview', label: 'Overview' },
		{ key: 'commands', label: 'Commands' },
		{ key: 'shadows', label: 'Shadows' },
		{ key: 'telemetry', label: 'Telemetry' }
	];
</script>

<div class="space-y-6">
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
		<!-- Header -->
		<div class="flex items-start justify-between">
			<div>
				<div class="flex items-center gap-3">
					<h1 class="font-mono text-2xl font-bold">{device.device_id}</h1>
					{#if heartbeatRecent}
						<span class="inline-block h-2.5 w-2.5 animate-pulse rounded-full bg-success" title="Heartbeat active"></span>
					{/if}
				</div>
				<p class="mt-1 text-text-muted">Fleet: {typeof device.metadata?.fleet === 'string' ? device.metadata.fleet : device.fleet_id}</p>
			</div>
			<StatusBadge status={device.status} />
		</div>

		<!-- Tab navigation -->
		<div class="flex gap-1 border-b border-border">
			{#each TABS as tab}
				<button
					onclick={() => (activeTab = tab.key)}
					class="border-b-2 px-4 py-2 text-sm font-medium transition {activeTab === tab.key ? 'border-primary text-primary' : 'border-transparent text-text-muted hover:text-text'}"
				>
					{tab.label}
				</button>
			{/each}
		</div>

		<!-- Tab content -->
		{#if activeTab === 'overview'}
			<div class="space-y-6">
				<!-- Device info cards -->
				<div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
					<div class="rounded-lg border border-border bg-white p-4">
						<dt class="text-xs font-medium uppercase text-text-muted">Hardware</dt>
						<dd class="mt-1 font-mono text-sm">{formatHardwareType(device.hardware_type)}</dd>
					</div>
					<div class="rounded-lg border border-border bg-white p-4">
						<dt class="text-xs font-medium uppercase text-text-muted">VIN</dt>
						<dd class="mt-1 font-mono text-sm">{device.vin ?? 'Not available'}</dd>
					</div>
					<div class="rounded-lg border border-border bg-white p-4">
						<dt class="text-xs font-medium uppercase text-text-muted">Last Heartbeat</dt>
						<dd class="mt-1 text-sm">
							{#if lastHeartbeat}
								{timeAgo(lastHeartbeat)}
								<span class="ml-1 text-xs text-text-muted">{new Date(lastHeartbeat).toLocaleString()}</span>
							{:else}
								Never
							{/if}
						</dd>
					</div>
					<div class="rounded-lg border border-border bg-white p-4">
						<dt class="text-xs font-medium uppercase text-text-muted">Certificate</dt>
						<dd class="mt-1 font-mono text-sm">{device.certificate_id ?? 'None'}</dd>
					</div>
					<div class="rounded-lg border border-border bg-white p-4">
						<dt class="text-xs font-medium uppercase text-text-muted">Created</dt>
						<dd class="mt-1 text-sm">{new Date(device.created_at).toLocaleDateString()}</dd>
					</div>
				</div>

				<!-- Service status row -->
				<div>
					<h2 class="mb-3 text-sm font-medium text-text-muted">Service Status</h2>
					<div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
						<ServiceIndicator label="Ollama" status={ollamaStatus} />
						<ServiceIndicator label="CAN Bus" status={canStatus} value={reported?.can_interface ? String(reported.can_interface) : undefined} />
						<ServiceIndicator label="Uptime" value={uptimeValue} />
						<ServiceIndicator label="Agent" value={agentVersion} />
					</div>
					{#if !stateShadow}
						<p class="mt-2 text-xs text-text-muted">No "state" shadow available — service status will appear when the agent reports.</p>
					{/if}
				</div>
			</div>

		{:else if activeTab === 'commands'}
			<div class="space-y-6">
				<section class="rounded-lg border border-border bg-white p-6">
					<h2 class="mb-4 text-lg font-semibold">Command Interface</h2>
					<CommandForm
						deviceId={device.device_id}
						fleetId={typeof device.metadata?.fleet === 'string'
							? device.metadata.fleet
							: typeof device.fleet_id === 'string'
								? device.fleet_id
								: ''}
					/>
				</section>

				<!-- Device command history -->
				<section>
					<div class="mb-3 flex items-center justify-between">
						<h2 class="text-lg font-semibold">Command History</h2>
						<button
							onclick={loadDeviceCommands}
							disabled={commandsLoading}
							class="rounded-md border border-border px-3 py-1 text-xs text-text-muted hover:text-text disabled:opacity-50"
						>
							Refresh
						</button>
					</div>

					{#if commandsLoading}
						<p class="text-sm text-text-muted">Loading commands...</p>
					{:else if commands.length === 0}
						<p class="text-sm text-text-muted">No commands sent to this device yet.</p>
					{:else}
						<div class="overflow-x-auto rounded-lg border border-border bg-white">
							<table class="w-full text-sm">
								<thead class="border-b border-border bg-surface text-left text-xs font-medium uppercase text-text-muted">
									<tr>
										<th class="px-4 py-2">Command</th>
										<th class="px-4 py-2">Status</th>
										<th class="px-4 py-2">Response</th>
										<th class="px-4 py-2">Time</th>
									</tr>
								</thead>
								<tbody class="divide-y divide-border">
									{#each commands as cmd (cmd.id)}
										<tr class="hover:bg-surface/50">
											<td class="px-4 py-2">{cmd.command}</td>
											<td class="px-4 py-2"><StatusBadge status={cmd.status} /></td>
											<td class="max-w-xs truncate px-4 py-2 text-text-muted">
												{#if cmd.response_text}
													{cmd.response_text}
												{:else if cmd.status === 'pending'}
													<span class="italic">awaiting...</span>
												{:else}
													—
												{/if}
											</td>
											<td class="whitespace-nowrap px-4 py-2 text-text-muted">
												{new Date(cmd.created_at).toLocaleString()}
											</td>
										</tr>
									{/each}
								</tbody>
							</table>
						</div>
					{/if}
				</section>
			</div>

		{:else if activeTab === 'shadows'}
			<ShadowPanel {deviceId} />

		{:else if activeTab === 'telemetry'}
			<TelemetryPanel {deviceId} />
		{/if}
	{/if}
</div>
