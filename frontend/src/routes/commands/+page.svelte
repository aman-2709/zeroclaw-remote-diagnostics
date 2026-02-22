<script lang="ts">
	import { api, ApiClientError } from '$lib/api/client';
	import type { CommandSummary } from '$lib/types';
	import StatusBadge from '$lib/components/StatusBadge.svelte';

	let commands = $state<CommandSummary[]>([]);
	let loading = $state(true);
	let error = $state<string | null>(null);

	async function loadCommands() {
		loading = true;
		error = null;
		try {
			commands = await api.listCommands();
		} catch (err) {
			error = err instanceof ApiClientError ? err.message : 'Failed to load commands';
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		loadCommands();
	});
</script>

<div class="space-y-6">
	<div class="flex items-center justify-between">
		<h1 class="text-2xl font-bold">Command History</h1>
		<button
			onclick={loadCommands}
			disabled={loading}
			class="rounded-md border border-border px-3 py-1.5 text-sm hover:bg-white disabled:opacity-50"
		>
			Refresh
		</button>
	</div>

	{#if loading}
		<p class="text-text-muted">Loading commands...</p>
	{:else if error}
		<div class="rounded-md border border-danger/20 bg-danger/5 p-4 text-sm text-danger">
			{error}
		</div>
	{:else if commands.length === 0}
		<p class="text-text-muted">No commands sent yet.</p>
	{:else}
		<div class="overflow-x-auto rounded-lg border border-border bg-white">
			<table class="w-full text-sm">
				<thead class="border-b border-border bg-surface text-left text-xs font-medium text-text-muted uppercase">
					<tr>
						<th class="px-4 py-3">Device</th>
						<th class="px-4 py-3">Command</th>
						<th class="px-4 py-3">Status</th>
						<th class="px-4 py-3">Time</th>
					</tr>
				</thead>
				<tbody class="divide-y divide-border">
					{#each commands as cmd (cmd.id)}
						<tr class="hover:bg-surface/50">
							<td class="px-4 py-3 font-mono">
								<a href="/devices/{cmd.device_id}" class="text-primary hover:underline">
									{cmd.device_id}
								</a>
							</td>
							<td class="px-4 py-3">{cmd.command}</td>
							<td class="px-4 py-3">
								<StatusBadge status={cmd.status} />
							</td>
							<td class="px-4 py-3 text-text-muted">
								{new Date(cmd.created_at).toLocaleString()}
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	{/if}
</div>
