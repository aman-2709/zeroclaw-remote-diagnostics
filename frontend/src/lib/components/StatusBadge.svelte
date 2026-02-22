<script lang="ts">
	import type { DeviceStatus, CommandStatus } from '$lib/types';

	let { status }: { status: DeviceStatus | CommandStatus | null } = $props();

	const colors: Record<string, string> = {
		online: 'bg-success/10 text-success',
		offline: 'bg-text-muted/10 text-text-muted',
		error: 'bg-danger/10 text-danger',
		provisioning: 'bg-warning/10 text-warning',
		pending: 'bg-text-muted/10 text-text-muted',
		sent: 'bg-primary/10 text-primary',
		received: 'bg-primary/10 text-primary',
		executing: 'bg-warning/10 text-warning',
		completed: 'bg-success/10 text-success',
		failed: 'bg-danger/10 text-danger'
	};

	const colorClass = $derived(status ? (colors[status] ?? 'bg-text-muted/10 text-text-muted') : 'bg-text-muted/10 text-text-muted');
</script>

<span class="inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-medium {colorClass}">
	{status ?? 'unknown'}
</span>
