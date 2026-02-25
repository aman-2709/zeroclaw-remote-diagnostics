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
	let responseData = $state<unknown | null>(null);
	let responseError = $state<string | null>(null);
	let elapsedSecs = $state(0);

	let unsub: (() => void) | null = null;
	let pollTimer: ReturnType<typeof setInterval> | null = null;
	let tickTimer: ReturnType<typeof setInterval> | null = null;

	const POLL_INTERVAL_MS = 3000;
	const TIMEOUT_MS = 60000;

	onMount(() => {
		return () => {
			cleanup();
		};
	});

	function cleanup() {
		unsub?.();
		unsub = null;
		if (pollTimer) { clearInterval(pollTimer); pollTimer = null; }
		if (tickTimer) { clearInterval(tickTimer); tickTimer = null; }
	}

	function handleResponse(text: string | null, data: unknown | null, status: string, errMsg: string | null = null) {
		cleanup();
		awaitingResponse = false;
		responseText = text;
		responseData = data;
		if (status === 'failed') {
			responseError = errMsg || 'Command execution failed on device';
		}
	}

	function waitForResponse(commandId: string) {
		cleanup();
		awaitingResponse = true;
		responseText = null;
		responseData = null;
		responseError = null;
		elapsedSecs = 0;

		const startTime = Date.now();

		// Tick counter for elapsed time display
		tickTimer = setInterval(() => {
			elapsedSecs = Math.floor((Date.now() - startTime) / 1000);
		}, 1000);

		// Strategy 1: WebSocket push (instant)
		unsub = wsStore.onEvent((event: WsEvent) => {
			if (event.type === 'command_response' && event.command_id === commandId) {
				handleResponse(event.response_text ?? null, event.response_data ?? null, event.status, event.error ?? null);
			}
		});

		// Strategy 2: Polling fallback (catches missed WS events)
		pollTimer = setInterval(async () => {
			// Timeout check
			if (Date.now() - startTime > TIMEOUT_MS) {
				cleanup();
				awaitingResponse = false;
				responseError = 'Response timed out — device may be offline or processing is slow.';
				return;
			}

			try {
				// getCommand returns CommandRecord but the backend shape varies
				// (in-memory vs DB). Normalize via unknown → plain object access.
				const raw: unknown = await api.getCommand(commandId);
				const obj = raw as Record<string, unknown>;

				// In-memory: { command, response: { status, ... }, created_at }
				// DB mode:   { id, status, response_text, response_data, error, ... }
				const resp = obj.response as Record<string, unknown> | undefined;
				const status = (resp?.status ?? obj.status) as string | undefined;
				const text = (resp?.response_text ?? obj.response_text) as string | null;
				const data = (resp?.response_data ?? obj.response_data) as unknown | null;
				const errMsg = (resp?.error ?? obj.error) as string | null;

				if (status && status !== 'pending' && status !== 'sent' && status !== 'received' && status !== 'executing') {
					handleResponse(text ?? null, data ?? null, status, errMsg ?? null);
				}
			} catch {
				// Poll failed — will retry next interval
			}
		}, POLL_INTERVAL_MS);
	}

	async function handleSubmit(e: Event) {
		e.preventDefault();
		if (!command.trim() || !deviceId) return;

		loading = true;
		error = null;
		lastResult = null;
		awaitingResponse = false;
		responseText = null;
		responseData = null;
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
			waitForResponse(envelope.id);
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

	function formatEntries(data: unknown): string[] | null {
		if (!data || typeof data !== 'object') return null;
		const obj = data as Record<string, unknown>;
		const inner = obj.data as Record<string, unknown> | undefined;
		const entries = inner?.entries;
		if (!Array.isArray(entries)) return null;
		return entries.map((e: Record<string, unknown>) => e.message as string).filter(Boolean);
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
					Waiting for device response... ({elapsedSecs}s)
				</div>
			{/if}

			{#if responseText}
				<div class="mt-2 rounded border border-success/20 bg-success/5 p-2 text-xs">
					<span class="font-medium text-success">Response:</span>
					<pre class="mt-1 whitespace-pre-wrap break-words font-mono text-text">{responseText}</pre>
				</div>
			{/if}

			{#if responseData}
				{@const logLines = formatEntries(responseData)}
				{#if logLines}
					<details class="mt-2 rounded border border-border bg-surface p-2 text-xs" open>
						<summary class="cursor-pointer font-medium text-text-muted">Log Entries ({logLines.length})</summary>
						<pre class="mt-1 max-h-80 overflow-auto whitespace-pre-wrap break-words font-mono text-text leading-relaxed">{logLines.join('\n')}</pre>
					</details>
				{:else}
					<details class="mt-2 rounded border border-border bg-surface p-2 text-xs">
						<summary class="cursor-pointer font-medium text-text-muted">Response Data</summary>
						<pre class="mt-1 max-h-64 overflow-auto whitespace-pre-wrap break-words font-mono text-text">{JSON.stringify(responseData, null, 2)}</pre>
					</details>
				{/if}
			{/if}

			{#if responseError}
				<div class="mt-2 rounded border border-danger/20 bg-danger/5 p-2 text-xs text-danger">
					{responseError}
				</div>
			{/if}
		</div>
	{/if}
</form>
