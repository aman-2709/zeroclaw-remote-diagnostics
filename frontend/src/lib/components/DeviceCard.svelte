<script lang="ts">
	import type { DeviceSummary } from '$lib/types';
	import { formatHardwareType } from '$lib/types/device';
	import StatusBadge from './StatusBadge.svelte';

	let { device }: { device: DeviceSummary } = $props();

	const timeAgo = $derived(() => {
		if (!device.last_heartbeat) return 'never';
		const diff = Date.now() - new Date(device.last_heartbeat).getTime();
		const mins = Math.floor(diff / 60000);
		if (mins < 1) return 'just now';
		if (mins < 60) return `${mins}m ago`;
		const hours = Math.floor(mins / 60);
		if (hours < 24) return `${hours}h ago`;
		return `${Math.floor(hours / 24)}d ago`;
	});
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
		Last heartbeat: {timeAgo()}
	</p>
</a>
