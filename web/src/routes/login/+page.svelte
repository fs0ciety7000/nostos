<script lang="ts">
	let { form } = $props();
</script>

<svelte:head><title>Nostos · Entrer</title></svelte:head>

<main>
	<form method="POST">
		<p class="mono k">Nostos · Atlas</p>
		<h1>Chambre noire</h1>
		<p class="lede">Nostos est privé. Colle le jeton propriétaire de ton instance pour entrer.</p>
		<label class="mono" for="token">Jeton propriétaire</label>
		<input id="token" name="token" type="password" autocomplete="current-password" required />
		{#if form?.message}<p class="err" role="alert">{form.message}</p>{/if}
		<button type="submit">Entrer</button>
	</form>
</main>

<style>
	main { min-height: 100dvh; display: grid; place-items: center; padding: 2rem max(16px, env(safe-area-inset-left)); }
	form { width: min(100%, 420px); display: grid; gap: 0.9rem; }
	h1 { font: italic 400 clamp(3rem, 9vw, 4.6rem) / 0.9 var(--f-display); margin: 0; letter-spacing: -0.03em; }
	.lede { margin: 0 0 0.6rem; color: var(--ink-2); }
	input { background: transparent; border: 1px solid var(--rule-2); color: var(--ink); padding: 0.7rem 0.8rem; font: 500 0.95rem var(--f-mono); }
	input:focus { border-color: var(--safelight); outline: none; }
	button { justify-self: start; background: var(--safelight); color: #1a0d08; border: 0; padding: 0.7rem 1.2rem; font: 500 0.72rem var(--f-mono); letter-spacing: 0.1em; text-transform: uppercase; cursor: pointer; }
	.err { margin: 0; color: #e6909f; font-size: 0.9rem; }
</style>
