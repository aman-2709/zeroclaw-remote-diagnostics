<script lang="ts">
	let { data, highlightKeys = [] }: { data: unknown; highlightKeys?: string[] } = $props();

	function isObject(v: unknown): v is Record<string, unknown> {
		return v !== null && typeof v === 'object' && !Array.isArray(v);
	}

	function formatPrimitive(v: unknown): string {
		if (v === null) return 'null';
		if (typeof v === 'string') return `"${v}"`;
		return String(v);
	}

	function primitiveColor(v: unknown): string {
		if (v === null) return 'text-text-muted';
		if (typeof v === 'string') return 'text-success';
		if (typeof v === 'number') return 'text-primary';
		if (typeof v === 'boolean') return 'text-warning';
		return 'text-text';
	}
</script>

{#snippet renderValue(value: unknown, depth: number)}
	{#if isObject(value)}
		{#if Object.keys(value).length === 0}
			<span class="text-text-muted">{'{}'}</span>
		{:else}
			<div class="pl-4" style="margin-left: {depth > 0 ? 0 : 0}px">
				{#each Object.entries(value) as [key, val]}
					{@const highlighted = highlightKeys.includes(key)}
					<div class="flex items-start gap-1 {highlighted ? 'rounded bg-warning/10 px-1 -mx-1' : ''}">
						<span class="shrink-0 font-semibold text-text">{key}:</span>
						{#if isObject(val) || Array.isArray(val)}
							{@render renderValue(val, depth + 1)}
						{:else}
							<span class={primitiveColor(val)}>{formatPrimitive(val)}</span>
						{/if}
					</div>
				{/each}
			</div>
		{/if}
	{:else if Array.isArray(value)}
		{#if value.length === 0}
			<span class="text-text-muted">[]</span>
		{:else}
			<div class="pl-4">
				{#each value as item, i}
					<div class="flex items-start gap-1">
						<span class="shrink-0 text-text-muted">[{i}]</span>
						{#if isObject(item) || Array.isArray(item)}
							{@render renderValue(item, depth + 1)}
						{:else}
							<span class={primitiveColor(item)}>{formatPrimitive(item)}</span>
						{/if}
					</div>
				{/each}
			</div>
		{/if}
	{:else}
		<span class={primitiveColor(value)}>{formatPrimitive(value)}</span>
	{/if}
{/snippet}

<div class="font-mono text-sm leading-relaxed">
	{@render renderValue(data, 0)}
</div>
