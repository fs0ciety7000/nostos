<script lang="ts">
	let { children } = $props();
</script>

<svelte:head>
	<link rel="preconnect" href="https://fonts.googleapis.com" />
	<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin="anonymous" />
	<link
		rel="stylesheet"
		href="https://fonts.googleapis.com/css2?family=Bodoni+Moda:ital,opsz,wght@0,6..96,400..700;1,6..96,400..700&family=Hanken+Grotesk:wght@400;500;600&family=JetBrains+Mono:wght@400;500&display=swap"
	/>
</svelte:head>

{@render children()}

<style>
	/* Chambre noire: one committed dark look (darkroom), no light theme. */
	:global(:root) {
		--bg: #0b0709; --bg-2: #130b10; --glass: rgba(11, 7, 9, 0.9);
		--ink: #efe3dc; --ink-2: #c3b1ae; --ink-3: #8f7a7f; --rule: #34232b; --rule-2: #4d3540;
		--safelight: #ff8a5c; --safelight-2: #ffb58c; --ember: #f2b36a; --plan: #7fc4d8;
		--f-display: 'Bodoni Moda', 'Didot', 'Bodoni 72', Georgia, serif;
		--f-body: 'Hanken Grotesk', system-ui, -apple-system, 'Segoe UI', sans-serif;
		--f-mono: 'JetBrains Mono', ui-monospace, 'SFMono-Regular', Menlo, monospace;
		--gut: max(16px, 2rem);
		color-scheme: dark;
	}
	:global(*) { box-sizing: border-box; }
	:global(html, body) { height: 100%; margin: 0; }
	:global(body) { background: var(--bg); color: var(--ink); font: 15px/1.55 var(--f-body); -webkit-font-smoothing: antialiased; }
	:global(button, input) { font: inherit; color: inherit; }
	:global(:focus-visible) { outline: 2px solid var(--safelight); outline-offset: 2px; }
	:global(.mono) { font-family: var(--f-mono); font-size: 0.68rem; letter-spacing: 0.1em; text-transform: uppercase; }
	:global(.k) { color: var(--safelight); }
	:global(h1, h2, h3) { font-family: var(--f-display); font-weight: 400; margin: 0; text-wrap: balance; }
	@media (prefers-reduced-motion: reduce) { :global(*) { transition: none !important; animation: none !important; } }
</style>
