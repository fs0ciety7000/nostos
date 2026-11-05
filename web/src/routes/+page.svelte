<script lang="ts">
	import { page } from '$app/state';
	import { replaceState } from '$app/navigation';
	import { onMount } from 'svelte';
	import Globe from '$lib/components/Globe.svelte';
	import Gallery from '$lib/components/Gallery.svelte';
	import type { Photo, Stop } from '$lib/types';

	let { data } = $props();

	let tripSlug = $state<string | null>(null);
	let stopId = $state<string | null>(null);
	let center = $state<{ lat: number; lon: number } | null>(null);
	let width = $state(1280);
	let photos = $state<Record<string, Photo[] | null>>({});
	let photoError = $state<string | null>(null);

	const narrow = $derived(width < 760);
	const detail = $derived(data.details.find((d) => d.trip.slug === tripSlug) ?? null);
	const stop = $derived<Stop | null>(detail?.stops.find((s) => s.id === stopId) ?? null);
	const padding = $derived(
		narrow
			? { left: 0, right: 0, bottom: stop ? Math.round(window.innerHeight * 0.62) : tripSlug ? 260 : 180 }
			: { left: stop ? 0 : Math.min(460, width * 0.36), right: stop ? Math.min(640, width * 0.48) : 0, bottom: 0 }
	);
	const stopPhotos = $derived(stop && tripSlug ? (photos[tripSlug] ?? null)?.filter((p) => p.stop_id === stop.id) ?? null : null);
	const named = $derived(detail?.stops.filter((s) => !s.is_waypoint) ?? []);

	onMount(() => {
		const q = page.url.searchParams;
		const t = q.get('trip');
		if (t && data.details.some((d) => d.trip.slug === t)) {
			tripSlug = t;
			stopId = q.get('stop');
		}
	});

	$effect(() => {
		const slug = tripSlug;
		if (!slug || photos[slug] !== undefined) return;
		photos[slug] = null;
		photoError = null;
		fetch(`/api/trips/${slug}/photos`)
			.then(async (r) => {
				if (!r.ok) throw new Error(String(r.status));
				const fc = await r.json();
				photos[slug] = fc.features.map((f: { properties: Photo }) => f.properties);
			})
			.catch(() => {
				photos[slug] = [];
				photoError = "Les vues n'ont pas pu être chargées. Recharge la page dans un instant.";
			});
	});

	$effect(() => {
		const q = new URLSearchParams();
		if (tripSlug) q.set('trip', tripSlug);
		if (stopId) q.set('stop', stopId);
		const url = q.size ? `?${q}` : page.url.pathname;
		try {
			replaceState(url, {});
		} catch {
			/* before hydration */
		}
	});

	function selectTrip(slug: string | null) {
		tripSlug = slug;
		stopId = null;
	}
	function selectStop(s: Stop) {
		const d = data.details.find((x) => x.stops.some((y) => y.id === s.id));
		tripSlug = d?.trip.slug ?? tripSlug;
		stopId = s.id;
	}
	function back() {
		if (stopId) stopId = null;
		else if (tripSlug) tripSlug = null;
	}
	const fmtDate = (d: string | null) => (d ? new Date(d + 'T12:00:00Z').toLocaleDateString('fr-FR', { month: 'short', year: 'numeric', timeZone: 'UTC' }) : '');
	const readout = $derived(center ? `${Math.abs(center.lat).toFixed(2)}° ${center.lat >= 0 ? 'N' : 'S'} · ${Math.abs(center.lon).toFixed(2)}° ${center.lon < 0 ? 'O' : 'E'}` : '—');
</script>

<svelte:head><title>Nostos · Atlas</title></svelte:head>
<svelte:window bind:innerWidth={width} onkeydown={(e) => e.key === 'Escape' && back()} />

<div class="app" class:has-trip={!!tripSlug} class:is-open={!!stop}>
	<header class="bar">
		<div class="brand"><b>Nostos</b><span class="mono k">Atlas</span></div>
		<span class="readout mono" aria-hidden="true">{readout}</span>
		<form method="POST" action="/logout"><button class="mono out" type="submit">Sortir</button></form>
	</header>

	<div class="stage">
		<Globe details={data.details} {tripSlug} {stop} {padding} onselecttrip={selectTrip} onselectstop={selectStop} onmove={(c) => (center = c)} />
		<div class="labels"></div>
		<div class="scrim"></div>

		<section class="side" aria-label="Odyssées">
			{#if data.apiError}
				<p class="banner" role="alert">{data.apiError}</p>
			{/if}
			<span class="mono k">{detail ? `Odyssée · ${detail.days.length} chant${detail.days.length > 1 ? 's' : ''}` : 'Tes odyssées'}</span>
			<h1 class="big">{detail ? detail.trip.title : 'Atlas'}</h1>
			{#if detail}
				<p class="lede">
					{detail.stops.length} escale{detail.stops.length > 1 ? 's' : ''} · {detail.photos.ready} vue{detail.photos.ready > 1 ? 's' : ''}{detail.route_km ? ` · ${Math.round(detail.route_km).toLocaleString('fr-FR')} km` : ''}{detail.film_simulations[0] ? ` · surtout en ${detail.film_simulations[0].name}` : ''}.
					{#if detail.photos.in_flight}<span class="note">{detail.photos.in_flight} encore en développement.</span>{/if}
				</p>
			{:else if data.trips.length}
				<p class="lede">Choisis une odyssée pour voir sa route et ses escales.</p>
			{:else if !data.apiError}
				<p class="lede">Aucune odyssée pour l'instant. Crée un voyage et envoie ses photos à l'API ; elles se poseront ici, sur le globe.</p>
			{/if}

			{#if data.trips.length}
				<div class="trips">
					{#each data.trips as t (t.id)}
						<button type="button" aria-pressed={tripSlug === t.slug} onclick={() => selectTrip(tripSlug === t.slug ? null : t.slug)}>
							<span class="t">{t.title}</span>
							<span class="mono">{fmtDate(t.starts_on)} · {t.photos} vues</span>
						</button>
					{/each}
				</div>
			{/if}
			{#if named.length}
				<div class="chips" aria-label="Escales">
					{#each named as s (s.id)}
						<button type="button" onclick={() => selectStop(s)} aria-pressed={s.id === stopId}>{s.name ?? `Escale ${s.seq}`}</button>
					{/each}
				</div>
			{/if}
		</section>

		<div class="legend mono"><span><i></i>Itinéraire</span><span><i class="dot"></i>Escale</span></div>
		<div class="hint mono">Glisse pour tourner · Échap pour revenir</div>

		{#if stop && detail}
			<Gallery trip={detail} {stop} photos={stopPhotos} error={photoError} onclose={back} />
		{/if}
		<div class="grain" aria-hidden="true"></div>
	</div>
</div>

<style>
	.app { height: 100dvh; min-height: 560px; display: grid; grid-template-rows: auto 1fr; background: radial-gradient(120% 90% at 70% 50%, #1a0e14 0%, #0b0709 60%, #070405 100%); }
	.bar { position: relative; z-index: 20; display: flex; align-items: center; gap: 1.5rem; padding: max(0.9rem, env(safe-area-inset-top)) var(--gut) 0.8rem; border-bottom: 1px solid var(--rule); }
	.brand { display: flex; gap: 0.7rem; align-items: baseline; }
	.brand b { font: 400 1.45rem var(--f-display); letter-spacing: -0.01em; }
	.readout { margin-left: auto; color: var(--ink-3); font-variant-numeric: tabular-nums; }
	.out { background: none; border: 1px solid var(--rule-2); padding: 0.35rem 0.6rem; cursor: pointer; color: var(--ink-2); }
	.stage { position: relative; overflow: hidden; isolation: isolate; }
	.labels { position: absolute; inset: 0; z-index: 2; pointer-events: none; }
	.labels :global(.pin) { position: absolute; left: 0; top: 0; opacity: 1; pointer-events: auto; border: 0; background: transparent; padding: 0.25rem 0.4rem; cursor: pointer; white-space: nowrap; color: #ffcfae; font: 500 0.62rem var(--f-mono); letter-spacing: 0.1em; text-transform: uppercase; transition: opacity 0.35s; will-change: transform; text-shadow: 0 1px 3px #000; }
	.labels :global(.pin[data-off='1']) { opacity: 0; pointer-events: none; }
	.labels :global(.pin::before) { content: ''; display: inline-block; width: 14px; height: 1px; vertical-align: middle; margin-right: 0.4rem; background: currentColor; opacity: 0.7; }
	.labels :global(.pin.sel) { color: #fff; }
	.labels :global(.pin.trip) { font: italic 400 1.1rem var(--f-display); letter-spacing: 0; text-transform: none; color: var(--ink); }
	.labels :global(.pin.trip small) { font: 500 0.56rem var(--f-mono); letter-spacing: 0.1em; text-transform: uppercase; margin-left: 0.45rem; opacity: 0.75; }
	.scrim { position: absolute; inset: 0 auto 0 0; width: 54%; z-index: 2; pointer-events: none; background: linear-gradient(90deg, rgba(9, 5, 7, 0.94) 0%, rgba(9, 5, 7, 0.72) 55%, rgba(9, 5, 7, 0) 100%); opacity: 0; transition: opacity 0.6s; }
	.has-trip:not(.is-open) .scrim { opacity: 1; }
	.side { position: absolute; z-index: 3; left: var(--gut); top: 50%; transform: translateY(-50%); width: min(36%, 440px); display: grid; grid-template-columns: minmax(0, 1fr); gap: 1.1rem; transition: opacity 0.5s, transform 0.8s cubic-bezier(0.2, 0.8, 0.2, 1); max-height: calc(100% - 4rem); overflow-y: auto; scrollbar-width: none; }
	.is-open .side { opacity: 0; transform: translate(-30px, -50%); pointer-events: none; }
	.big { font: italic 400 clamp(3rem, 6.4vw, 6rem) / 0.88 var(--f-display); letter-spacing: -0.03em; overflow-wrap: anywhere; }
	.lede { margin: 0; color: var(--ink-2); font-size: 0.95rem; max-width: 38ch; }
	.note { color: var(--ember); }
	.banner { margin: 0; border: 1px solid #6b3a44; color: #e6909f; padding: 0.6rem 0.8rem; font-size: 0.9rem; }
	.trips { display: grid; border-top: 1px solid var(--rule); }
	.trips button { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 1rem; align-items: baseline; background: transparent; border: 0; border-bottom: 1px solid var(--rule); padding: 0.75rem 0; text-align: left; cursor: pointer; }
	.trips .t { font: 400 1.35rem var(--f-display); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
	.trips button[aria-pressed='true'] .t { font-style: italic; color: var(--safelight-2); }
	.trips .mono { color: var(--ink-3); white-space: nowrap; }
	.chips { display: flex; flex-wrap: wrap; gap: 0.35rem; }
	.chips button { border: 1px solid var(--rule-2); background: rgba(255, 255, 255, 0.02); padding: 0.3rem 0.65rem; border-radius: 99px; cursor: pointer; font-size: 0.78rem; }
	.chips button:hover { border-color: var(--safelight); }
	.legend { position: absolute; z-index: 3; left: var(--gut); bottom: 1.3rem; display: flex; gap: 1.2rem; color: var(--ink-3); pointer-events: none; }
	.legend i { display: inline-block; width: 20px; border-top: 2px solid var(--safelight); vertical-align: middle; margin-right: 0.4rem; }
	.legend i.dot { width: 8px; height: 8px; border: 1.5px solid #ffd2b0; border-radius: 50%; }
	.hint { position: absolute; z-index: 3; right: var(--gut); bottom: 2.9rem; color: #7d676c; pointer-events: none; }
	.is-open .hint { display: none; }
	.grain { position: absolute; inset: -50%; z-index: 12; pointer-events: none; mix-blend-mode: overlay; opacity: 0.07; background-image: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='220' height='220'><filter id='n'><feTurbulence type='fractalNoise' baseFrequency='.85' numOctaves='2' stitchTiles='stitch'/><feColorMatrix values='0 0 0 0 .5 0 0 0 0 .5 0 0 0 0 .5 0 0 0 1.6 0'/></filter><rect width='100%' height='100%' filter='url(%23n)'/></svg>"); animation: grain 1.1s steps(5) infinite; }
	@keyframes grain { 0% { transform: translate(0, 0); } 20% { transform: translate(-3%, 2%); } 40% { transform: translate(2%, -3%); } 60% { transform: translate(-2%, -1%); } 80% { transform: translate(3%, 3%); } 100% { transform: translate(0, 0); } }
	@media (max-width: 760px) {
		.readout { display: none; }
		.brand { margin-right: auto; }
		.side { top: auto; bottom: 0; left: 0; right: 0; width: auto; transform: none; padding: 1rem 16px 1.2rem; max-height: 52%; background: linear-gradient(180deg, rgba(11, 7, 9, 0), rgba(11, 7, 9, 0.95) 3rem); }
		.is-open .side { transform: translateY(20px); }
		.big { font-size: 3rem; }
		.scrim, .legend, .hint { display: none; }
	}
</style>
