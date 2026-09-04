<script lang="ts">
	import { onMount } from 'svelte';
	import { api, ApiClientError } from '$lib/api/client';
	import type { CommandSummary, DeviceSummary, WsEvent } from '$lib/types';
	import DeviceCard from '$lib/components/DeviceCard.svelte';
	import StatusBadge from '$lib/components/StatusBadge.svelte';
	import { wsStore } from '$lib/stores/websocket.svelte';
	import { timeAgo } from '$lib/utils/format';

	let devices = $state<DeviceSummary[]>([]);
	let commands = $state<CommandSummary[]>([]);
	let loading = $state(true);
	let commandsLoading = $state(true);
	let error = $state<string | null>(null);
	let lastUpdated = $state<Date | null>(null);

	const onlineCount = $derived(devices.filter((device) => device.status === 'online').length);
	const offlineCount = $derived(devices.filter((device) => device.status === 'offline').length);
	const attentionCount = $derived(devices.filter((device) => device.status === 'error').length);
	const activeCommands = $derived(commands.filter((command) => ['pending', 'sent', 'received', 'executing'].includes(command.status ?? '')).length);
	const completedCommands = $derived(commands.filter((command) => command.status === 'completed').length);
	const failedCommands = $derived(commands.filter((command) => command.status === 'failed').length);
	const recentCommands = $derived(commands.slice(0, 5));

	async function loadDashboard() {
		loading = true;
		error = null;
		try {
			devices = await api.listDevices();
			lastUpdated = new Date();
		} catch (err) {
			error = err instanceof ApiClientError ? err.message : 'Unable to reach the fleet API';
		} finally {
			loading = false;
		}
	}

	async function loadCommands() {
		commandsLoading = true;
		try {
			commands = await api.listCommands();
		} catch {
			// The device overview remains useful when command history is unavailable.
		} finally {
			commandsLoading = false;
		}
	}

	onMount(() => {
		loadDashboard();
		loadCommands();
		const unsub = wsStore.onEvent((event: WsEvent) => {
			if (event.type === 'device_heartbeat') {
				devices = devices.map((device) => device.device_id === event.device_id
					? { ...device, status: 'online', last_heartbeat: event.timestamp }
					: device);
				lastUpdated = new Date();
			} else if (event.type === 'device_status_changed') {
				devices = devices.map((device) => device.device_id === event.device_id
					? { ...device, status: event.new_status as DeviceSummary['status'] }
					: device);
			} else if (event.type === 'command_dispatched') {
				commands = [{ id: event.command_id, device_id: event.device_id, command: event.command, status: 'pending', created_at: event.created_at }, ...commands];
			} else if (event.type === 'command_response') {
				commands = commands.map((command) => command.id === event.command_id
					? { ...command, status: event.status as CommandSummary['status'], response_text: event.response_text }
					: command);
			}
		});
		return unsub;
	});
</script>

<svelte:head><title>Fleet overview · ZeroClaw</title></svelte:head>

<div class="space-y-8">
	<section class="flex flex-col justify-between gap-5 sm:flex-row sm:items-end">
		<div>
			<div class="eyebrow">Fleet overview</div>
			<h1 class="mt-2 text-3xl font-bold tracking-tight text-text sm:text-4xl">Good morning, operator</h1>
			<p class="mt-2 max-w-2xl text-sm leading-6 text-text-muted">Monitor device health, dispatch diagnostics, and keep your fleet moving.</p>
		</div>
		<div class="flex items-center gap-3">
			{#if lastUpdated}<span class="hidden text-xs text-text-muted sm:inline">Updated {timeAgo(lastUpdated.toISOString())}</span>{/if}
			<button onclick={() => { loadDashboard(); loadCommands(); }} disabled={loading} class="inline-flex items-center gap-2 rounded-lg border border-border bg-white px-3.5 py-2 text-sm font-semibold text-text shadow-sm transition hover:border-indigo-300 hover:text-primary disabled:cursor-wait disabled:opacity-60">
				<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="h-4 w-4 {loading ? 'animate-spin' : ''}"><path d="M20 11a8.1 8.1 0 0 0-14.9-4M4 5v4h4m-4 4a8.1 8.1 0 0 0 14.9 4M20 19v-4h-4" /></svg>
				Refresh
			</button>
		</div>
	</section>

	{#if error}
		<div role="alert" class="flex items-start gap-3 rounded-xl border border-danger/20 bg-danger/5 p-4 text-sm text-danger">
			<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="mt-0.5 h-5 w-5 shrink-0"><circle cx="12" cy="12" r="9"/><path d="M12 8v4m0 4h.01"/></svg>
			<div><div class="font-semibold">Fleet data unavailable</div><div class="mt-1 text-danger/80">{error}. Check that the API is running and try again.</div></div>
		</div>
	{/if}

	<section class="grid gap-4 sm:grid-cols-2 xl:grid-cols-4" aria-label="Fleet metrics">
		<div class="metric-card"><div class="flex items-center justify-between"><span class="eyebrow">Total devices</span><span class="flex h-8 w-8 items-center justify-center rounded-lg bg-indigo-50 text-indigo-600"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" class="h-4 w-4"><rect x="4" y="3" width="16" height="18" rx="2"/><path d="M8 7h8M8 11h8M8 15h4"/></svg></span></div><div class="mt-4 text-3xl font-bold tracking-tight">{loading ? '—' : devices.length}</div><div class="mt-2 text-xs text-text-muted">Registered in this workspace</div></div>
		<div class="metric-card"><div class="flex items-center justify-between"><span class="eyebrow">Online now</span><span class="flex h-8 w-8 items-center justify-center rounded-lg bg-emerald-50 text-success"><span class="h-2.5 w-2.5 rounded-full bg-success"></span></span></div><div class="mt-4 text-3xl font-bold tracking-tight text-success">{loading ? '—' : onlineCount}</div><div class="mt-2 text-xs text-text-muted">{devices.length ? Math.round((onlineCount / devices.length) * 100) : 0}% fleet availability</div></div>
		<div class="metric-card"><div class="flex items-center justify-between"><span class="eyebrow">Needs attention</span><span class="flex h-8 w-8 items-center justify-center rounded-lg bg-amber-50 text-warning"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" class="h-4 w-4"><path d="m12 4 9 16H3L12 4Z"/><path d="M12 9v5m0 3h.01"/></svg></span></div><div class="mt-4 text-3xl font-bold tracking-tight text-warning">{loading ? '—' : offlineCount + attentionCount}</div><div class="mt-2 text-xs text-text-muted">{attentionCount} reporting an error</div></div>
		<div class="metric-card"><div class="flex items-center justify-between"><span class="eyebrow">Command queue</span><span class="flex h-8 w-8 items-center justify-center rounded-lg bg-violet-50 text-violet-600"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" class="h-4 w-4"><path d="M5 4h14v16H5zM8 8h8M8 12h5M8 16h3"/></svg></span></div><div class="mt-4 text-3xl font-bold tracking-tight">{commandsLoading ? '—' : activeCommands}</div><div class="mt-2 text-xs text-text-muted">{completedCommands} completed · {failedCommands} failed</div></div>
	</section>

	<section class="grid gap-6 xl:grid-cols-[minmax(0,1.45fr)_minmax(340px,.75fr)]">
		<div class="surface-card overflow-hidden">
			<div class="flex items-center justify-between border-b border-border px-5 py-4 sm:px-6">
				<div><h2 class="font-bold text-text">Your devices</h2><p class="mt-1 text-xs text-text-muted">Live status from the last heartbeat</p></div>
				<a href="/commands" class="text-xs font-bold text-primary hover:text-primary-dark">View commands <span aria-hidden="true">→</span></a>
			</div>
			<div class="p-5 sm:p-6">
				{#if loading}
					<div class="grid gap-4 sm:grid-cols-2"><div class="h-32 animate-pulse rounded-xl bg-slate-100"></div><div class="h-32 animate-pulse rounded-xl bg-slate-100"></div></div>
				{:else if devices.length === 0}
					<div class="py-10 text-center"><div class="mx-auto flex h-11 w-11 items-center justify-center rounded-full bg-slate-100 text-slate-500"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" class="h-5 w-5"><rect x="4" y="5" width="16" height="14" rx="2"/><path d="M8 9h8m-8 4h5"/></svg></div><h3 class="mt-3 text-sm font-bold">No devices registered</h3><p class="mt-1 text-xs text-text-muted">Provision a device to start monitoring your fleet.</p></div>
				{:else}
					<div class="grid gap-4 sm:grid-cols-2">{#each devices as device (device.device_id)}<DeviceCard {device} />{/each}</div>
				{/if}
			</div>
		</div>

		<div class="surface-card overflow-hidden">
			<div class="flex items-center justify-between border-b border-border px-5 py-4 sm:px-6"><div><h2 class="font-bold text-text">Recent activity</h2><p class="mt-1 text-xs text-text-muted">Latest command activity</p></div><a href="/commands" class="text-xs font-bold text-primary hover:text-primary-dark">All activity <span aria-hidden="true">→</span></a></div>
			<div class="divide-y divide-border">
				{#if commandsLoading}
					{#each [1, 2, 3] as _}<div class="flex gap-3 px-5 py-4"><div class="h-8 w-8 animate-pulse rounded-lg bg-slate-100"></div><div class="flex-1"><div class="h-3 w-3/4 animate-pulse rounded bg-slate-100"></div><div class="mt-2 h-2.5 w-1/2 animate-pulse rounded bg-slate-100"></div></div></div>{/each}
				{:else if recentCommands.length === 0}
					<div class="px-5 py-12 text-center text-xs text-text-muted">No command activity yet.</div>
				{:else}
					{#each recentCommands as cmd (cmd.id)}
						<a href="/devices/{cmd.device_id}" class="flex items-start gap-3 px-5 py-4 transition hover:bg-slate-50"><div class="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-indigo-50 text-indigo-600"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" class="h-4 w-4"><path d="m8 9 3 3-3 3m5 0h3M5 4h14a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2Z"/></svg></div><div class="min-w-0 flex-1"><div class="truncate text-sm font-semibold text-text">{cmd.command}</div><div class="mt-1 flex items-center gap-2 text-xs text-text-muted"><span class="font-mono">{cmd.device_id}</span><span>·</span><span>{timeAgo(cmd.created_at)}</span></div></div><StatusBadge status={cmd.status} /></a>
					{/each}
				{/if}
			</div>
		</div>
	</section>
</div>
