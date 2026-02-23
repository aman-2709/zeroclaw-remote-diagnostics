<script lang="ts">
	import { api } from '$lib/api/client';
	import type { TelemetryReading, TelemetrySource, WsEvent } from '$lib/types';
	import { shortDateTime } from '$lib/utils/format';
	import { wsStore } from '$lib/stores/websocket.svelte';
	import { onMount } from 'svelte';
	import SparklineChart from './SparklineChart.svelte';

	let { deviceId }: { deviceId: string } = $props();

	type SourceFilter = 'all' | TelemetrySource;

	let readings = $state<TelemetryReading[]>([]);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let sourceFilter = $state<SourceFilter>('all');

	const SOURCE_LABELS: Record<SourceFilter, string> = {
		all: 'All',
		obd2: 'OBD-II',
		system: 'System',
		canbus: 'CAN Bus'
	};

	const CHART_COLORS: Record<TelemetrySource, string> = {
		obd2: '#3b82f6',
		system: '#10b981',
		canbus: '#f59e0b'
	};

	const filtered = $derived(
		sourceFilter === 'all' ? readings : readings.filter((r) => r.source === sourceFilter)
	);

	// Group numeric readings by metric_name
	const numericGroups = $derived(() => {
		const groups = new Map<string, { time: string; value: number; source: TelemetrySource; unit: string | null }[]>();
		for (const r of filtered) {
			if (r.value_numeric !== null) {
				let arr = groups.get(r.metric_name);
				if (!arr) {
					arr = [];
					groups.set(r.metric_name, arr);
				}
				arr.push({ time: r.time, value: r.value_numeric, source: r.source, unit: r.unit });
			}
		}
		return groups;
	});

	// Non-numeric readings
	const textReadings = $derived(filtered.filter((r) => r.value_numeric === null));

	async function loadTelemetry() {
		loading = true;
		error = null;
		try {
			const resp = await api.getTelemetry(deviceId, undefined, 200);
			readings = resp.readings;
		} catch {
			error = 'Failed to load telemetry';
		} finally {
			loading = false;
		}
	}

	onMount(() => {
		loadTelemetry();

		const unsub = wsStore.onEvent((event: WsEvent) => {
			if (event.type === 'telemetry_ingested' && event.device_id === deviceId) {
				loadTelemetry();
			}
		});

		return unsub;
	});
</script>

<div class="space-y-4">
	<!-- Source filter tabs -->
	<div class="flex gap-1 rounded-lg bg-surface p-1">
		{#each Object.entries(SOURCE_LABELS) as [key, label]}
			<button
				onclick={() => (sourceFilter = key as SourceFilter)}
				class="rounded-md px-3 py-1 text-sm transition {sourceFilter === key ? 'bg-white font-medium text-text shadow-sm' : 'text-text-muted hover:text-text'}"
			>
				{label}
			</button>
		{/each}
	</div>

	{#if loading}
		<p class="text-sm text-text-muted">Loading telemetry...</p>
	{:else if error}
		<p class="text-sm text-danger">{error}</p>
	{:else if filtered.length === 0}
		<div class="rounded-lg border border-border bg-white p-8 text-center">
			<p class="text-sm text-text-muted">No telemetry data available.</p>
			<p class="mt-1 text-xs text-text-muted">Readings will appear here when the device reports telemetry.</p>
		</div>
	{:else}
		<!-- Numeric metric charts -->
		{#if numericGroups().size > 0}
			<div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
				{#each [...numericGroups().entries()] as [metricName, points]}
					<SparklineChart
						points={points.map((p) => ({ time: p.time, value: p.value }))}
						label={metricName}
						unit={points[0]?.unit ?? ''}
						color={points[0] ? CHART_COLORS[points[0].source] : '#3b82f6'}
					/>
				{/each}
			</div>
		{/if}

		<!-- Text/JSON readings table -->
		{#if textReadings.length > 0}
			<div class="rounded-lg border border-border bg-white">
				<table class="w-full text-sm">
					<thead>
						<tr class="border-b border-border bg-surface/50 text-left text-xs font-medium uppercase text-text-muted">
							<th class="px-3 py-2">Time</th>
							<th class="px-3 py-2">Metric</th>
							<th class="px-3 py-2">Value</th>
							<th class="px-3 py-2">Source</th>
						</tr>
					</thead>
					<tbody>
						{#each textReadings.slice(0, 50) as reading}
							<tr class="border-b border-border last:border-none">
								<td class="whitespace-nowrap px-3 py-2 font-mono text-xs text-text-muted">{shortDateTime(reading.time)}</td>
								<td class="px-3 py-2 font-mono text-xs">{reading.metric_name}</td>
								<td class="max-w-xs truncate px-3 py-2 text-xs">
									{#if reading.value_text}
										{reading.value_text}
									{:else if reading.value_json !== null}
										<code class="text-xs">{JSON.stringify(reading.value_json)}</code>
									{:else}
										<span class="text-text-muted">—</span>
									{/if}
								</td>
								<td class="px-3 py-2 text-xs text-text-muted">{reading.source}</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		{/if}
	{/if}
</div>
