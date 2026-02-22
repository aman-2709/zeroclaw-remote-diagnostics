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
	let lastResult = $state<CommandEnvelope | null>(null);

	async function handleSubmit(e: Event) {
		e.preventDefault();
		if (!command.trim() || !deviceId) return;

		loading = true;
		error = null;
		lastResult = null;

		try {
			const envelope = await api.sendCommand({
				device_id: deviceId,
				fleet_id: fleetId || 'default',
				command: command.trim(),
				initiated_by: 'dashboard-user'
			});
			lastResult = envelope;
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

	{#if lastResult}
		<div class="rounded-md border border-border bg-surface p-3 text-sm">
			<div class="flex items-center gap-2 text-success font-medium">
				Command dispatched
				<span class="font-mono text-xs text-text-muted">{lastResult.id}</span>
			</div>
			{#if lastResult.parsed_intent}
				<div class="mt-2 space-y-1 text-xs">
					<div>
						<span class="text-text-muted">Tool:</span>
						<span class="font-mono font-medium">{lastResult.parsed_intent.tool_name}</span>
					</div>
					{#if Object.keys(lastResult.parsed_intent.tool_args).length > 0}
						<div>
							<span class="text-text-muted">Args:</span>
							<span class="font-mono">{JSON.stringify(lastResult.parsed_intent.tool_args)}</span>
						</div>
					{/if}
					<div>
						<span class="text-text-muted">Confidence:</span>
						<span class="font-mono">{(lastResult.parsed_intent.confidence * 100).toFixed(0)}%</span>
					</div>
				</div>
			{:else}
				<p class="mt-1 text-xs text-warning">Command could not be parsed — will require cloud inference.</p>
			{/if}
		</div>
	{/if}
</form>
