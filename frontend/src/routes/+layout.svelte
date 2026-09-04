<script lang="ts">
	import '../app.css';
	import { page } from '$app/stores';
	import { wsStore } from '$lib/stores/websocket.svelte';
	import { onMount } from 'svelte';

	let { children } = $props();

	onMount(() => {
		wsStore.connect();
		return () => wsStore.disconnect();
	});

	const connectionLabel = $derived(
		wsStore.status === 'connected' ? 'Live connection' : wsStore.status === 'connecting' ? 'Connecting' : 'Offline'
	);
	const connectionClass = $derived(
		wsStore.status === 'connected' ? 'bg-success' : wsStore.status === 'connecting' ? 'bg-warning' : 'bg-danger'
	);
</script>

<svelte:head>
	<title>ZeroClaw · Fleet Operations</title>
	<meta name="description" content="Real-time fleet diagnostics and device operations console." />
	<link rel="preconnect" href="https://fonts.googleapis.com" />
	<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin="anonymous" />
	<link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700;800&family=JetBrains+Mono:wght@500;600&display=swap" rel="stylesheet" />
</svelte:head>

<div class="app-shell flex">
	<aside class="app-sidebar hidden min-h-screen flex-col md:flex">
		<div class="flex h-20 items-center gap-3 border-b border-white/8 px-6">
			<div class="flex h-9 w-9 items-center justify-center rounded-xl bg-indigo-500 text-sm font-extrabold tracking-tight text-white shadow-lg shadow-indigo-950/30">ZC</div>
			<div class="brand-label">
				<div class="text-sm font-bold tracking-tight text-white">ZeroClaw</div>
				<div class="mt-0.5 text-[10px] font-medium uppercase tracking-[.16em] text-slate-500">Fleet operations</div>
			</div>
		</div>

		<nav class="flex-1 px-3 py-3" aria-label="Primary navigation">
			<div class="sidebar-section">Workspace</div>
			<a href="/" class:active={$page.url.pathname === '/'} class="sidebar-link" aria-current={$page.url.pathname === '/' ? 'page' : undefined}>
				<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" class="h-4 w-4"><path d="M3 13h8V3H3v10Zm0 8h8v-5H3v5Zm10 0h8V11h-8v10Zm0-18v5h8V3h-8Z" /></svg>
				<span>Fleet overview</span>
			</a>
			<a href="/commands" class:active={$page.url.pathname.startsWith('/commands')} class="sidebar-link" aria-current={$page.url.pathname.startsWith('/commands') ? 'page' : undefined}>
				<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" class="h-4 w-4"><path d="m8 9 3 3-3 3m5 0h3M5 4h14a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2Z" /></svg>
				<span>Command history</span>
			</a>

			<div class="sidebar-section">Operations</div>
			<div class="sidebar-link cursor-default opacity-55" title="Coming soon">
				<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" class="h-4 w-4"><path d="M4 19V5m0 14h16M8 16v-4m4 4V8m4 8V5" /></svg>
				<span>Fleet analytics <small class="ml-auto text-[9px] font-medium normal-case tracking-normal">Soon</small></span>
			</div>
			<div class="sidebar-link cursor-default opacity-55" title="Coming soon">
				<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" class="h-4 w-4"><path d="M12 3v18m9-9H3m15.36-6.36-12.72 12.72m0-12.72 12.72 12.72" /></svg>
				<span>Automations <small class="ml-auto text-[9px] font-medium normal-case tracking-normal">Soon</small></span>
			</div>
		</nav>

		<div class="border-t border-white/8 p-3">
			<div class="rounded-xl bg-white/5 p-3">
				<div class="flex items-center gap-2">
					<span class="h-2 w-2 rounded-full {connectionClass}"></span>
					<span class="text-xs font-semibold text-slate-300">{connectionLabel}</span>
				</div>
				<p class="mt-2 text-[11px] leading-4 text-slate-500">Real-time device events and command responses.</p>
			</div>
		</div>
	</aside>

	<div class="min-w-0 flex-1">
		<header class="sticky top-0 z-10 flex h-16 items-center justify-between border-b border-border bg-white/90 px-4 backdrop-blur sm:px-7">
			<div class="flex items-center gap-3">
				<div class="flex h-8 w-8 items-center justify-center rounded-lg bg-indigo-600 text-[11px] font-extrabold text-white md:hidden">ZC</div>
				<div>
					<div class="text-[11px] font-bold uppercase tracking-[.12em] text-text-muted">Operations console</div>
					<div class="hidden text-sm font-semibold text-text sm:block">Fleet command center</div>
				</div>
			</div>
			<div class="flex items-center gap-3">
				<span class="hidden items-center gap-2 text-xs font-medium text-text-muted sm:flex">
					<span class="h-2 w-2 rounded-full {connectionClass} {wsStore.status === 'connected' ? 'animate-pulse' : ''}"></span>
					{connectionLabel}
				</span>
				<div class="h-7 w-px bg-border"></div>
				<div class="flex h-8 w-8 items-center justify-center rounded-full bg-indigo-50 text-xs font-bold text-indigo-700" title="Dashboard operator">DO</div>
			</div>
		</header>

		<main class="mx-auto min-h-[calc(100vh-4rem)] max-w-[1440px] px-4 py-7 sm:px-7 lg:px-10 lg:py-9">
			{@render children()}
		</main>
	</div>
</div>
