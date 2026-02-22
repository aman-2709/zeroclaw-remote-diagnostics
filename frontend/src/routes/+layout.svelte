<script lang="ts">
	import '../app.css';

	let { children } = $props();
</script>

<div class="min-h-screen bg-surface text-text">
	<header class="border-b border-border bg-white">
		<nav class="mx-auto flex max-w-7xl items-center justify-between px-6 py-3">
			<a href="/" class="flex items-center gap-2 font-bold text-primary">
				<span class="rounded bg-primary px-2 py-0.5 font-mono text-sm text-white">ZC</span>
				<span>Fleet Dashboard</span>
			</a>
			<div class="flex gap-6 text-sm">
				<a href="/" class="text-text-muted hover:text-text">Devices</a>
				<a href="/commands" class="text-text-muted hover:text-text">Commands</a>
			</div>
		</nav>
	</header>

	<main class="mx-auto max-w-7xl px-6 py-8">
		{@render children()}
	</main>
</div>
