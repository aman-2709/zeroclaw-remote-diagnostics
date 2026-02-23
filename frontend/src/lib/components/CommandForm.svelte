<script lang="ts">
	import { api } from '$lib/api/client';
	import type { CommandEnvelope, WsEvent } from '$lib/types';
	import { wsStore } from '$lib/stores/websocket.svelte';
	import { onMount } from 'svelte';

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
	let awaitingResponse = $state(false);
	let responseText = $state<string | null>(null);
	let responseError = $state<string | null>(null);

	let unsub: (() => void) | null = null;

	onMount(() => {
		return () => {
			unsub?.();
		};
	});

	function subscribeToResponse(commandId: string) {
		unsub?.();
		awaitingResponse = true;
		responseText = null;
		responseError = null;

		unsub = wsStore.onEvent((event: WsEvent) => {
			if (event.type === 'command_response' && event.command_id === commandId) {
				awaitingResponse = false;
				responseText = event.response_text ?? null;
				if (event.status === 'failed') {
					responseError = 'Command execution failed on device';
				}
				unsub?.();
				unsub = null;
			}
		});
	}

	async function handleSubmit(e: Event) {
		e.preventDefault();
		if (!command.trim() || !deviceId) return;

		loading = true;
		error = null;
		lastResult = null;
		awaitingResponse = false;
		responseText = null;
		responseError = null;

		try {
			const envelope = await api.sendCommand({
				device_id: deviceId,
				fleet_id: fleetId || 'default',
				command: command.trim(),
				initiated_by: 'dashboard-user'
			});
			lastResult = envelope;
			command = '';
			subscribeToResponse(envelope.id);
			onSuccess?.(envelope);
		} catch (err) {
			error = err instanceof Error ? err.message : 'Failed to send command';
		} finally {
			loading = false;
		}
	}

	function actionLabel(action?: string): string {
		switch (action) {
			case 'shell':
				return 'Shell';
			case 'reply':
				return 'Reply';
			case 'tool':
			default:
				return 'Tool';
		}
	}

	function actionColor(action?: string): string {
		switch (action) {
			case 'shell':
				return 'text-blue-400';
			case 'reply':
				return 'text-purple-400';
			case 'tool':
			default:
				return 'text-success';
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
				placeholder="e.g. read DTCs, what's the CPU temp, how are you?"
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
						<span class="text-text-muted">Action:</span>
						<span class="font-mono font-medium {actionColor(lastResult.parsed_intent.action)}">{actionLabel(lastResult.parsed_intent.action)}</span>
					</div>
					{#if lastResult.parsed_intent.action !== 'reply'}
						<div>
							<span class="text-text-muted">{lastResult.parsed_intent.action === 'shell' ? 'Command:' : 'Tool:'}</span>
							<span class="font-mono font-medium">{lastResult.parsed_intent.tool_name}</span>
						</div>
					{/if}
					{#if lastResult.parsed_intent.action !== 'shell' && lastResult.parsed_intent.action !== 'reply' && Object.keys(lastResult.parsed_intent.tool_args).length > 0}
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
				<p class="mt-1 text-xs text-text-muted">Command sent to device for processing.</p>
			{/if}

			{#if awaitingResponse}
				<div class="mt-2 flex items-center gap-2 text-xs text-text-muted">
					<span class="inline-block h-3 w-3 animate-pulse rounded-full bg-warning"></span>
					Waiting for device response...
				</div>
			{/if}

			{#if responseText}
				<div class="mt-2 rounded border border-success/20 bg-success/5 p-2 text-xs">
					<span class="font-medium text-success">Response:</span>
					<pre class="mt-1 whitespace-pre-wrap break-words font-mono text-text">{responseText}</pre>
				</div>
			{/if}

			{#if responseError}
				<div class="mt-2 rounded border border-danger/20 bg-danger/5 p-2 text-xs text-danger">
					{responseError}
				</div>
			{/if}
		</div>
	{/if}
</form>
