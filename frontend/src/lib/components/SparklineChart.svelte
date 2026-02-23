<script lang="ts">
	import { shortDateTime } from '$lib/utils/format';

	let {
		points,
		label,
		unit = '',
		color = '#3b82f6'
	}: {
		points: { time: string; value: number }[];
		label: string;
		unit?: string;
		color?: string;
	} = $props();

	const WIDTH = 280;
	const HEIGHT = 80;
	const PADDING = { top: 8, right: 8, bottom: 8, left: 8 };

	const chartW = WIDTH - PADDING.left - PADDING.right;
	const chartH = HEIGHT - PADDING.top - PADDING.bottom;

	const sorted = $derived(
		[...points].sort((a, b) => new Date(a.time).getTime() - new Date(b.time).getTime())
	);

	const yMin = $derived(sorted.length > 0 ? Math.min(...sorted.map((p) => p.value)) : 0);
	const yMax = $derived(sorted.length > 0 ? Math.max(...sorted.map((p) => p.value)) : 1);
	const yRange = $derived(yMax === yMin ? 1 : yMax - yMin);

	const polylinePoints = $derived(
		sorted
			.map((p, i) => {
				const x = PADDING.left + (sorted.length > 1 ? (i / (sorted.length - 1)) * chartW : chartW / 2);
				const y = PADDING.top + chartH - ((p.value - yMin) / yRange) * chartH;
				return `${x},${y}`;
			})
			.join(' ')
	);

	const fillPoints = $derived(() => {
		if (sorted.length === 0) return '';
		const baseline = PADDING.top + chartH;
		const firstX = PADDING.left + (sorted.length > 1 ? 0 : chartW / 2);
		const lastX = PADDING.left + (sorted.length > 1 ? chartW : chartW / 2);
		return `${firstX},${baseline} ${polylinePoints} ${lastX},${baseline}`;
	});

	const lastValue = $derived(sorted.length > 0 ? sorted[sorted.length - 1].value : null);

	let hoverIndex = $state<number | null>(null);

	function handleMouseMove(e: MouseEvent) {
		const svg = (e.currentTarget as SVGSVGElement).getBoundingClientRect();
		const relX = e.clientX - svg.left - PADDING.left;
		const idx = Math.round((relX / chartW) * (sorted.length - 1));
		hoverIndex = Math.max(0, Math.min(sorted.length - 1, idx));
	}

	function handleMouseLeave() {
		hoverIndex = null;
	}

	const hoverPoint = $derived(hoverIndex !== null ? sorted[hoverIndex] : null);
	const hoverX = $derived(
		hoverIndex !== null && sorted.length > 1
			? PADDING.left + (hoverIndex / (sorted.length - 1)) * chartW
			: null
	);
	const hoverY = $derived(
		hoverPoint
			? PADDING.top + chartH - ((hoverPoint.value - yMin) / yRange) * chartH
			: null
	);
</script>

<div class="rounded-lg border border-border bg-white p-3">
	<div class="mb-1 flex items-baseline justify-between">
		<span class="text-xs font-medium text-text-muted">{label}</span>
		{#if lastValue !== null}
			<span class="font-mono text-sm font-semibold">
				{lastValue.toFixed(1)}{unit ? ` ${unit}` : ''}
			</span>
		{/if}
	</div>

	{#if sorted.length < 2}
		<p class="py-4 text-center text-xs text-text-muted">Not enough data points</p>
	{:else}
		<!-- svelte-ignore a11y_no_static_element_interactions -->
		<svg
			width="100%"
			viewBox="0 0 {WIDTH} {HEIGHT}"
			preserveAspectRatio="none"
			class="overflow-visible"
			onmousemove={handleMouseMove}
			onmouseleave={handleMouseLeave}
		>
			<!-- Fill area -->
			<polygon points={fillPoints()} fill={color} opacity="0.08" />

			<!-- Line -->
			<polyline points={polylinePoints} fill="none" stroke={color} stroke-width="1.5" />

			<!-- Y-axis labels -->
			<text x={PADDING.left} y={PADDING.top - 1} font-size="8" fill="#9ca3af">{yMax.toFixed(1)}</text>
			<text x={PADDING.left} y={PADDING.top + chartH + 8} font-size="8" fill="#9ca3af">{yMin.toFixed(1)}</text>

			<!-- Hover indicator -->
			{#if hoverX !== null && hoverY !== null && hoverPoint}
				<line x1={hoverX} y1={PADDING.top} x2={hoverX} y2={PADDING.top + chartH} stroke="#d1d5db" stroke-width="1" />
				<circle cx={hoverX} cy={hoverY} r="3" fill={color} />
			{/if}
		</svg>

		<!-- Hover tooltip -->
		{#if hoverPoint}
			<div class="mt-1 text-center font-mono text-xs text-text-muted">
				{hoverPoint.value.toFixed(2)}{unit ? ` ${unit}` : ''} &middot; {shortDateTime(hoverPoint.time)}
			</div>
		{/if}
	{/if}
</div>
