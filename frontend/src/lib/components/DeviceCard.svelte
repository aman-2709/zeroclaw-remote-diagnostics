<script lang="ts">
	import type { DeviceSummary } from '$lib/types';
	import { formatHardwareType } from '$lib/types/device';
	import { timeAgo } from '$lib/utils/format';
	import StatusBadge from './StatusBadge.svelte';

	let { device }: { device: DeviceSummary } = $props();

	const heartbeatAgo = $derived(device.last_heartbeat ? timeAgo(device.last_heartbeat) : 'never');
</script>

<a
	href="/devices/{device.device_id}"
	class="block rounded-lg border border-border bg-white p-4 transition hover:border-primary hover:shadow-sm"
>
	<div class="flex items-start justify-between">
		<div>
			<h3 class="font-mono font-semibold">{device.device_id}</h3>
			<p class="mt-1 text-sm text-text-muted">
				{formatHardwareType(device.hardware_type)}
			</p>
		</div>
		<StatusBadge status={device.status} />
	</div>
	<p class="mt-3 text-xs text-text-muted">
		Last heartbeat: {heartbeatAgo}
	</p>
</a>
