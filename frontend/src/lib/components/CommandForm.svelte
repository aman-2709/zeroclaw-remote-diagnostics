<script lang="ts">
	import { api } from '$lib/api/client';
	import type { CommandEnvelope } from '$lib/types';

	let {
		deviceId = '',
		fleetId = '',
		onSuccess
	}: {
		deviceId?: string;
		fleetId?: string;
		onSuccess?: (envelope: CommandEnvelope) => void;
	} = $props();

	let command = $state('');
	let loading = $state(false);
	let error = $state<string | null>(null);
	let success = $state<string | null>(null);

	async function handleSubmit(e: Event) {
		e.preventDefault();
		if (!command.trim() || !deviceId) return;

		loading = true;
		error = null;
		success = null;

		try {
			const envelope = await api.sendCommand({
				device_id: deviceId,
				fleet_id: fleetId || 'default',
				command: command.trim(),
				initiated_by: 'dashboard-user'
			});
			success = `Command sent (${envelope.id})`;
			command = '';
			onSuccess?.(envelope);
		} catch (err) {
			error = err instanceof Error ? err.message : 'Failed to send command';
		} finally {
			loading = false;
		}
	}
</script>

<form onsubmit={handleSubmit} class="space-y-3">
	<div>
		<label for="command-input" class="block text-sm font-medium text-text">
			Send Command to <span class="font-mono">{deviceId || '...'}</span>
		</label>
		<div class="mt-1 flex gap-2">
			<input
				id="command-input"
				type="text"
				bind:value={command}
				placeholder="e.g. read DTCs, check engine RPM, tail syslog"
				disabled={loading || !deviceId}
				class="flex-1 rounded-md border border-border px-3 py-2 text-sm outline-none focus:border-primary focus:ring-1 focus:ring-primary disabled:opacity-50"
			/>
			<button
				type="submit"
				disabled={loading || !command.trim() || !deviceId}
				class="rounded-md bg-primary px-4 py-2 text-sm font-medium text-white hover:bg-primary-dark disabled:opacity-50"
			>
				{loading ? 'Sending...' : 'Send'}
			</button>
		</div>
	</div>

	{#if error}
		<p class="text-sm text-danger">{error}</p>
	{/if}

	{#if success}
		<p class="text-sm text-success">{success}</p>
	{/if}
</form>
