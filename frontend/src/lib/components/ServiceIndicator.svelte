<script lang="ts">
	let {
		label,
		status,
		value
	}: {
		label: string;
		status?: string;
		value?: string;
	} = $props();

	const dotColor = $derived(() => {
		switch (status) {
			case 'running':
				return 'bg-success';
			case 'stopped':
				return 'bg-text-muted';
			case 'error':
				return 'bg-danger';
			default:
				return 'bg-warning';
		}
	});
</script>

<div class="rounded-lg border border-border bg-white p-3">
	<div class="flex items-center gap-2">
		{#if status}
			<span class="inline-block h-2 w-2 shrink-0 rounded-full {dotColor()}"></span>
		{/if}
		<span class="text-xs font-medium uppercase text-text-muted">{label}</span>
	</div>
	{#if value}
		<p class="mt-1 font-mono text-sm font-semibold">{value}</p>
	{:else if status}
		<p class="mt-1 text-sm capitalize">{status}</p>
	{/if}
</div>
