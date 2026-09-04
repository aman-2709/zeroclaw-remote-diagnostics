<script lang="ts">
	import type { DeviceStatus, CommandStatus } from '$lib/types';

	let { status }: { status: DeviceStatus | CommandStatus | null } = $props();
	const labels: Record<string, string> = { online: 'Online', offline: 'Offline', error: 'Error', provisioning: 'Provisioning', pending: 'Pending', sent: 'Sent', received: 'Received', executing: 'Running', completed: 'Completed', failed: 'Failed' };
	const tones: Record<string, string> = {
		online: 'status-success', completed: 'status-success',
		offline: 'status-neutral', pending: 'status-neutral',
		sent: 'status-info', received: 'status-info',
		provisioning: 'status-warning', executing: 'status-warning',
		error: 'status-danger', failed: 'status-danger'
	};
	const tone = $derived(tones[status ?? ''] ?? 'status-neutral');
	const label = $derived(labels[status ?? ''] ?? 'Unknown');
</script>

<span class="inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 text-[11px] font-bold {tone}">
	<span class="h-1.5 w-1.5 rounded-full bg-current"></span>{label}
</span>

<style>
	:global(.status-success) { background: #ecfdf3; color: #087f5b; }
	:global(.status-neutral) { background: #f2f4f7; color: #667085; }
	:global(.status-info) { background: #eef4ff; color: #3f5bd8; }
	:global(.status-warning) { background: #fff7e6; color: #a15c00; }
	:global(.status-danger) { background: #fff0f1; color: #c23945; }
</style>
