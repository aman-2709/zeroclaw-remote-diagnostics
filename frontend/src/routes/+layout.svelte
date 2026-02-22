<script lang="ts">
	import '../app.css';
	import { wsStore } from '$lib/stores/websocket.svelte';
	import { onMount } from 'svelte';

	let { children } = $props();

	onMount(() => {
		wsStore.connect();
		return () => wsStore.disconnect();
	});

	const statusColor = $derived(
		wsStore.status === 'connected'
			? 'bg-success'
			: wsStore.status === 'connecting'
				? 'bg-warning'
				: 'bg-danger'
	);
</script>

<div class="min-h-screen bg-surface text-text">
	<header class="border-b border-border bg-white">
		<nav class="mx-auto flex max-w-7xl items-center justify-between px-6 py-3">
			<a href="/" class="flex items-center gap-2 font-bold text-primary">
				<span class="rounded bg-primary px-2 py-0.5 font-mono text-sm text-white">ZC</span>
				<span>Fleet Dashboard</span>
			</a>
			<div class="flex items-center gap-6 text-sm">
				<a href="/" class="text-text-muted hover:text-text">Devices</a>
				<a href="/commands" class="text-text-muted hover:text-text">Commands</a>
				<span class="flex items-center gap-1.5 text-xs text-text-muted" title="WebSocket {wsStore.status}">
					<span class="inline-block h-2 w-2 rounded-full {statusColor}"></span>
					{wsStore.status === 'connected' ? 'Live' : wsStore.status === 'connecting' ? 'Connecting' : 'Offline'}
				</span>
			</div>
		</nav>
	</header>

	<main class="mx-auto max-w-7xl px-6 py-8">
		{@render children()}
	</main>
</div>
