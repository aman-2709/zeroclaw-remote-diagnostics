<script lang="ts">
	import type { DeviceSummary } from '$lib/types';
	import { formatHardwareType } from '$lib/types/device';
	import { timeAgo } from '$lib/utils/format';
	import StatusBadge from './StatusBadge.svelte';

	let { device }: { device: DeviceSummary } = $props();
	const heartbeatAgo = $derived(device.last_heartbeat ? timeAgo(device.last_heartbeat) : 'Never');
	const isOnline = $derived(device.status === 'online');
</script>

<a href="/devices/{device.device_id}" class="group block rounded-xl border border-border bg-white p-4 transition duration-150 hover:-translate-y-0.5 hover:border-indigo-200 hover:shadow-lg hover:shadow-indigo-950/5 focus-visible:-translate-y-0.5 sm:p-5">
	<div class="flex items-start justify-between gap-3">
		<div class="flex min-w-0 items-center gap-3">
			<div class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl {isOnline ? 'bg-emerald-50 text-success' : 'bg-slate-100 text-slate-500'}">
				<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" class="h-5 w-5"><rect x="5" y="4" width="14" height="16" rx="2"/><path d="M9 8h6M9 12h6M9 16h3"/></svg>
			</div>
			<div class="min-w-0"><h3 class="truncate font-mono text-sm font-semibold text-text">{device.device_id}</h3><p class="mt-1 truncate text-xs text-text-muted">{formatHardwareType(device.hardware_type)}</p></div>
		</div>
		<StatusBadge status={device.status} />
	</div>
	<div class="mt-5 flex items-center justify-between border-t border-border pt-3 text-xs">
		<span class="flex items-center gap-1.5 text-text-muted"><span class="h-1.5 w-1.5 rounded-full {isOnline ? 'bg-success' : 'bg-slate-300'}"></span>{isOnline ? 'Reporting normally' : 'Last seen'}</span>
		<span class="font-medium text-text-muted">{heartbeatAgo}</span>
	</div>
	<div class="mt-3 flex items-center justify-between text-[11px] font-semibold text-primary opacity-0 transition group-hover:opacity-100"><span>Open device</span><span aria-hidden="true">→</span></div>
</a>
