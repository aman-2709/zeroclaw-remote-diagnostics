<script lang="ts">
	import { onMount } from 'svelte';
	import { api, ApiClientError } from '$lib/api/client';
	import type { CommandSummary, WsEvent } from '$lib/types';
	import StatusBadge from '$lib/components/StatusBadge.svelte';
	import { wsStore } from '$lib/stores/websocket.svelte';
	import { timeAgo } from '$lib/utils/format';

	let commands = $state<CommandSummary[]>([]);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let search = $state('');
	let statusFilter = $state<'all' | 'pending' | 'completed' | 'failed'>('all');

	const filteredCommands = $derived(commands.filter((command) => {
		const query = search.trim().toLowerCase();
		const matchesSearch = !query || command.command.toLowerCase().includes(query) || command.device_id.toLowerCase().includes(query);
		const matchesStatus = statusFilter === 'all' || command.status === statusFilter;
		return matchesSearch && matchesStatus;
	}));
	const inFlight = $derived(commands.filter((command) => ['pending', 'sent', 'received', 'executing'].includes(command.status ?? '')).length);
	const successful = $derived(commands.filter((command) => command.status === 'completed').length);
	const unsuccessful = $derived(commands.filter((command) => command.status === 'failed').length);

	async function loadCommands() {
		loading = true;
		error = null;
		try { commands = await api.listCommands(); }
		catch (err) { error = err instanceof ApiClientError ? err.message : 'Unable to reach the fleet API'; }
		finally { loading = false; }
	}

	onMount(() => {
		loadCommands();
		const unsub = wsStore.onEvent((event: WsEvent) => {
			if (event.type === 'command_dispatched') {
				commands = [{ id: event.command_id, device_id: event.device_id, command: event.command, status: 'pending', created_at: event.created_at }, ...commands];
			} else if (event.type === 'command_response') {
				commands = commands.map((command) => command.id === event.command_id ? { ...command, status: event.status as CommandSummary['status'], response_text: event.response_text } : command);
			}
		});
		return unsub;
	});
</script>

<svelte:head><title>Command history · ZeroClaw</title></svelte:head>

<div class="space-y-7">
	<section class="flex flex-col justify-between gap-5 sm:flex-row sm:items-end">
		<div><div class="eyebrow">Operations</div><h1 class="mt-2 text-3xl font-bold tracking-tight text-text">Command history</h1><p class="mt-2 text-sm text-text-muted">Review every diagnostic request sent to your fleet.</p></div>
		<button onclick={loadCommands} disabled={loading} class="inline-flex items-center justify-center gap-2 rounded-lg border border-border bg-white px-3.5 py-2 text-sm font-semibold text-text shadow-sm transition hover:border-indigo-300 hover:text-primary disabled:opacity-60"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="h-4 w-4 {loading ? 'animate-spin' : ''}"><path d="M20 11a8.1 8.1 0 0 0-14.9-4M4 5v4h4m-4 4a8.1 8.1 0 0 0 14.9 4M20 19v-4h-4"/></svg>Refresh</button>
	</section>

	<section class="grid gap-3 sm:grid-cols-3" aria-label="Command metrics">
		<div class="metric-card"><div class="eyebrow">In flight</div><div class="mt-3 text-2xl font-bold text-warning">{inFlight}</div><div class="mt-1 text-xs text-text-muted">Awaiting a device response</div></div>
		<div class="metric-card"><div class="eyebrow">Completed</div><div class="mt-3 text-2xl font-bold text-success">{successful}</div><div class="mt-1 text-xs text-text-muted">Successfully processed</div></div>
		<div class="metric-card"><div class="eyebrow">Failed</div><div class="mt-3 text-2xl font-bold text-danger">{unsuccessful}</div><div class="mt-1 text-xs text-text-muted">Require investigation</div></div>
	</section>

	<section class="surface-card overflow-hidden">
		<div class="flex flex-col gap-3 border-b border-border p-4 sm:flex-row sm:items-center sm:justify-between sm:px-5">
			<div class="relative min-w-0 flex-1 sm:max-w-sm"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" class="pointer-events-none absolute left-3 top-2.5 h-4 w-4 text-slate-400"><circle cx="10.8" cy="10.8" r="6.8"/><path d="m16 16 4.5 4.5"/></svg><input bind:value={search} aria-label="Search commands" placeholder="Search device or command" class="w-full rounded-lg border border-border bg-slate-50 py-2 pl-9 pr-3 text-sm outline-none transition placeholder:text-slate-400 focus:border-indigo-400 focus:bg-white focus:ring-2 focus:ring-indigo-100" /></div>
			<div class="flex rounded-lg bg-slate-100 p-1" role="group" aria-label="Filter command status">
				{#each [['all', 'All'], ['pending', 'In flight'], ['completed', 'Completed'], ['failed', 'Failed']] as option}
					<button onclick={() => (statusFilter = option[0] as typeof statusFilter)} class="rounded-md px-2.5 py-1.5 text-xs font-semibold transition {statusFilter === option[0] ? 'bg-white text-text shadow-sm' : 'text-text-muted hover:text-text'}">{option[1]}</button>
				{/each}
			</div>
		</div>

		{#if error}<div role="alert" class="m-5 rounded-lg border border-danger/20 bg-danger/5 p-3 text-sm text-danger">{error}</div>{/if}
		{#if loading}
			<div class="space-y-3 p-5">{#each [1, 2, 3, 4] as _}<div class="flex gap-4"><div class="h-10 w-24 animate-pulse rounded bg-slate-100"></div><div class="h-10 flex-1 animate-pulse rounded bg-slate-100"></div><div class="h-10 w-20 animate-pulse rounded bg-slate-100"></div></div>{/each}</div>
		{:else if filteredCommands.length === 0}
			<div class="px-5 py-16 text-center"><div class="mx-auto flex h-12 w-12 items-center justify-center rounded-full bg-slate-100 text-slate-500"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" class="h-5 w-5"><path d="m8 9 3 3-3 3m5 0h3M5 4h14a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2Z"/></svg></div><h2 class="mt-3 text-sm font-bold">{search || statusFilter !== 'all' ? 'No matching commands' : 'No commands yet'}</h2><p class="mt-1 text-xs text-text-muted">{search || statusFilter !== 'all' ? 'Try adjusting your search or filter.' : 'Commands sent from a device page will appear here.'}</p></div>
		{:else}
			<div class="overflow-x-auto"><table class="w-full min-w-[720px] text-left"><thead class="border-b border-border bg-slate-50/80"><tr><th class="px-5 py-3 text-[10px] font-extrabold uppercase tracking-[.08em] text-text-muted">Device</th><th class="px-5 py-3 text-[10px] font-extrabold uppercase tracking-[.08em] text-text-muted">Request</th><th class="px-5 py-3 text-[10px] font-extrabold uppercase tracking-[.08em] text-text-muted">Status</th><th class="px-5 py-3 text-[10px] font-extrabold uppercase tracking-[.08em] text-text-muted">Response</th><th class="px-5 py-3 text-[10px] font-extrabold uppercase tracking-[.08em] text-text-muted">Created</th></tr></thead><tbody class="divide-y divide-border">
				{#each filteredCommands as cmd (cmd.id)}<tr class="table-row"><td class="px-5 py-4"><a href="/devices/{cmd.device_id}" class="font-mono text-xs font-semibold text-primary hover:text-primary-dark">{cmd.device_id}</a></td><td class="max-w-[280px] px-5 py-4"><div class="truncate text-sm font-semibold text-text">{cmd.command}</div><div class="mt-1 font-mono text-[10px] text-slate-400">{cmd.id.slice(0, 8)}…</div></td><td class="px-5 py-4"><StatusBadge status={cmd.status} /></td><td class="max-w-[300px] px-5 py-4"><div class="truncate text-xs text-text-muted" title={cmd.response_text ?? ''}>{cmd.response_text ?? (cmd.status === 'pending' || cmd.status === 'executing' ? 'Waiting for device…' : '—')}</div></td><td class="whitespace-nowrap px-5 py-4"><div class="text-xs font-medium text-text">{timeAgo(cmd.created_at)}</div><div class="mt-1 text-[10px] text-slate-400">{new Date(cmd.created_at).toLocaleString()}</div></td></tr>{/each}
			</tbody></table></div>
		{/if}
	</section>
</div>
