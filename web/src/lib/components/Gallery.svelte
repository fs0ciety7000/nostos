<script lang="ts">
	// Gallery of one stop. Its colours come from the photos themselves: the
	// accent is the most saturated colour of the lead photo's palette, or the
	// tone of its film simulation.
	import { thumbHashToDataURL } from 'thumbhash';
	import { exposureLine, recipeRows, toneFor } from '$lib/philtre';
	import type { Photo, Stop, TripDetail } from '$lib/types';

	interface Props {
		trip: TripDetail;
		stop: Stop;
		photos: Photo[] | null;
		error: string | null;
		onclose: () => void;
	}
	let { trip, stop, photos, error, onclose }: Props = $props();

	let film = $state<string | null>(null);
	let lead = $state(0);
	let panel: HTMLElement;

	const shown = $derived((photos ?? []).filter((p) => !film || p.film_simulation === film));
	const films = $derived([...new Set((photos ?? []).map((p) => p.film_simulation).filter((f): f is string => !!f))]);
	const current = $derived(shown[Math.min(lead, Math.max(0, shown.length - 1))] ?? null);
	const tone = $derived(toneFor(current?.film_simulation ?? stop.film_simulation, current?.palette));
	const chant = $derived.by(() => {
		const day = current?.captured_local?.slice(0, 10) ?? (photos ?? []).find((p) => p.captured_local)?.captured_local?.slice(0, 10);
		return trip.days.find((d) => d.date === day)?.chant ?? null;
	});
	function roman(n: number): string {
		let out = '';
		for (const [v, r] of [[50, 'L'], [40, 'XL'], [10, 'X'], [9, 'IX'], [5, 'V'], [4, 'IV'], [1, 'I']] as const) {
			while (n >= v) {
				out += r;
				n -= v;
			}
		}
		return out;
	}
	function b64(s: string): Uint8Array {
		return Uint8Array.from(atob(s), (c) => c.charCodeAt(0));
	}
	function placeholder(p: Photo): string | undefined {
		try {
			return p.thumbhash ? thumbHashToDataURL(b64(p.thumbhash)) : undefined;
		} catch {
			return undefined;
		}
	}
	const when = (p: Photo) =>
		p.captured_local ? new Date(p.captured_local + 'Z').toLocaleString('fr-FR', { day: 'numeric', month: 'long', hour: '2-digit', minute: '2-digit', timeZone: 'UTC' }) : null;
	const coords = (lat: number, lon: number) => `${Math.abs(lat).toFixed(4)} ${lat >= 0 ? 'N' : 'S'} · ${Math.abs(lon).toFixed(4)} ${lon < 0 ? 'O' : 'E'}`;

	$effect(() => {
		void stop.id;
		film = null;
		lead = 0;
		panel?.scrollTo({ top: 0 });
	});

	function onkey(e: KeyboardEvent) {
		if (!shown.length) return;
		if (e.key === 'ArrowRight') lead = (lead + 1) % shown.length;
		else if (e.key === 'ArrowLeft') lead = (lead - 1 + shown.length) % shown.length;
	}
</script>

<svelte:window onkeydown={onkey} />

<aside class="panel" bind:this={panel} aria-label="Galerie de l'escale {stop.name ?? stop.seq}" style="--accent: {tone.accent}; --ground: {tone.ground}">
	<button type="button" class="close" onclick={onclose} aria-label="Retour à l'odyssée">×</button>
	<span class="mono kick">{chant ? `Chant ${roman(chant)} · ` : ''}{trip.trip.title}{current && when(current) ? ` · ${when(current)}` : ''}</span>
	<h2>{stop.name ?? `Escale ${stop.seq}`}</h2>
	<p class="mono sub">{coords(stop.lat, stop.lon)} · {stop.photos} vue{stop.photos > 1 ? 's' : ''}</p>

	{#if error}
		<p class="state" role="alert">{error}</p>
	{:else if photos === null}
		<div class="lead skeleton" aria-busy="true"></div>
		<p class="mono state">Développement des vues…</p>
	{:else if !shown.length}
		<p class="state">Aucune vue prête pour cette escale. Les photos encore en traitement apparaîtront ici.</p>
	{:else}
		{#if films.length > 1}
			<div class="chips" role="group" aria-label="Filtrer par simulation de film">
				<button type="button" aria-pressed={film === null} onclick={() => { film = null; lead = 0; }}>Toutes</button>
				{#each films as f (f)}
					<button type="button" aria-pressed={film === f} onclick={() => { film = f; lead = 0; }}>{f}</button>
				{/each}
			</div>
		{/if}

		{#key current?.id}
			<figure class="lead" style:background-image={current && placeholder(current) ? `url(${placeholder(current)})` : undefined} style:aspect-ratio={current?.width && current?.height ? `${current.width} / ${current.height}` : '3 / 2'}>
				{#if current?.preview_url}<img src={current.preview_url} alt="{stop.name ?? 'Escale'}, vue {lead + 1} sur {shown.length}" />{/if}
				<figcaption class="mono">{lead + 1} ▸ {shown.length}{current?.film_simulation ? ` · ${current.film_simulation}` : ''}</figcaption>
			</figure>
		{/key}

		{#if shown.length > 1}
			<div class="strip" role="group" aria-label="Vues de l'escale">
				{#each shown as p, i (p.id)}
					<button type="button" aria-pressed={i === lead} aria-label="Vue {i + 1}" onclick={() => (lead = i)} style:background-image={placeholder(p) ? `url(${placeholder(p)})` : undefined}>
						{#if p.thumb_url}<img src={p.thumb_url} alt="" loading="lazy" />{/if}
					</button>
				{/each}
			</div>
		{/if}

		{#if current}
			{#if current.palette?.length}
				<div class="palette" aria-label="Couleurs dominantes de la vue">
					{#each current.palette as c (c)}<span style:background={c} title={c}></span>{/each}
				</div>
			{/if}
			<dl class="meta">
				<div><dt>Prise de vue</dt><dd class="num">{exposureLine(current) || '—'}</dd></div>
				<div><dt>Boîtier</dt><dd>{[current.camera, current.lens].filter(Boolean).join(' · ') || '—'}</dd></div>
				<div><dt>Philtre</dt><dd>{current.film_simulation ?? 'Non lu'}</dd></div>
				<div><dt>Position</dt><dd class="num">{current.lat !== null && current.lon !== null ? coords(current.lat, current.lon) : 'Sans GPS · placée par l’heure'}</dd></div>
			</dl>
			{#if recipeRows(current.recipe).length}
				<section class="recipe" aria-label="Recette">
					<h3 class="mono">Recette</h3>
					<dl>
						{#each recipeRows(current.recipe) as [k, v] (k)}<dt>{k}</dt><dd>{v}</dd>{/each}
					</dl>
				</section>
			{/if}
		{/if}
	{/if}
</aside>

<style>
	.panel {
		position: absolute; z-index: 4; top: 0; right: 0; bottom: 0; width: min(48%, 640px);
		padding: 1.5rem var(--gut) 2rem 2rem; overflow: hidden auto; scrollbar-width: thin; scrollbar-color: var(--rule-2) transparent;
		background: linear-gradient(90deg, color-mix(in srgb, var(--ground) 55%, transparent), color-mix(in srgb, var(--ground) 96%, transparent) 16%);
		backdrop-filter: blur(6px); transition: --accent 0.7s, background 0.7s;
		animation: enter 0.8s cubic-bezier(0.2, 0.8, 0.2, 1) both;
	}
	@keyframes enter { from { opacity: 0; transform: translateX(40px); } }
	.close { position: sticky; top: 0; float: right; width: 2.2rem; height: 2.2rem; border: 1px solid currentColor; background: var(--glass); cursor: pointer; font-size: 1.15rem; line-height: 1; z-index: 2; }
	.kick { color: var(--accent); transition: color 0.7s; }
	h2 { font: italic 400 clamp(2.2rem, 4vw, 3.4rem) / 0.95 var(--f-display); margin: 0.35rem 0 0.3rem; letter-spacing: -0.02em; }
	.sub { color: var(--ink-3); margin: 0 0 1rem; font-variant-numeric: tabular-nums; }
	.state { color: var(--ink-2); max-width: 44ch; }
	.chips { display: flex; flex-wrap: wrap; gap: 0.3rem; margin-bottom: 0.9rem; }
	.chips button { border: 1px solid var(--rule-2); background: transparent; padding: 0.3rem 0.55rem; cursor: pointer; font: 500 0.62rem var(--f-mono); letter-spacing: 0.07em; text-transform: uppercase; color: var(--ink-2); }
	.chips button[aria-pressed='true'] { background: var(--accent); border-color: var(--accent); color: #120a08; }
	.lead { position: relative; margin: 0 0 0.5rem; background: #050304 center / cover; box-shadow: 0 30px 60px -20px rgba(0, 0, 0, 0.8); max-height: 62vh; overflow: hidden; }
	.lead img { display: block; width: 100%; height: 100%; object-fit: contain; animation: develop 0.9s ease both; }
	@keyframes develop { from { opacity: 0; filter: brightness(1.6) sepia(0.4); } }
	.lead figcaption { position: absolute; left: 0.6rem; bottom: 0.45rem; color: var(--ember); text-shadow: 0 1px 2px #000; }
	.skeleton { aspect-ratio: 3 / 2; background: linear-gradient(90deg, #140c10, #1d1217, #140c10); background-size: 200% 100%; animation: shimmer 1.4s linear infinite; }
	@keyframes shimmer { to { background-position: -200% 0; } }
	.strip { display: flex; gap: 5px; overflow-x: auto; background: #050304; padding: 16px 5px; position: relative; margin-bottom: 1rem; scrollbar-width: thin; }
	.strip::before, .strip::after { content: ''; position: absolute; left: 0; right: 0; height: 7px; background: radial-gradient(circle, #2a1d22 0 2.2px, transparent 2.6px) 0 0 / 12px 7px repeat-x; pointer-events: none; }
	.strip::before { top: 4px; } .strip::after { bottom: 4px; }
	.strip button { flex: 0 0 auto; width: 92px; aspect-ratio: 3 / 2; border: 0; padding: 0; background: #1d1217 center / cover; cursor: pointer; opacity: 0.55; transition: opacity 0.3s; outline-offset: -2px; }
	.strip button[aria-pressed='true'], .strip button:hover { opacity: 1; }
	.strip button[aria-pressed='true'] { box-shadow: inset 0 0 0 2px var(--accent); }
	.strip img { width: 100%; height: 100%; object-fit: cover; display: block; }
	.palette { display: flex; height: 10px; margin-bottom: 1rem; }
	.palette { gap: 2px; }
	.palette span { flex: 1; box-shadow: inset 0 0 0 1px rgba(239, 227, 220, 0.14); }
	.meta { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 0.7rem 1rem; margin: 0 0 1.2rem; }
	.meta div { border-top: 1px solid var(--rule); padding-top: 0.35rem; min-width: 0; }
	.meta dt { font: 500 0.56rem var(--f-mono); letter-spacing: 0.1em; text-transform: uppercase; color: var(--ink-3); }
	.meta dd { margin: 0; font-size: 0.86rem; overflow-wrap: anywhere; }
	.num { font-family: var(--f-mono); font-size: 0.76rem !important; font-variant-numeric: tabular-nums; }
	.recipe { border: 1px solid var(--rule); padding: 0.8rem 0.9rem; }
	.recipe h3 { color: var(--accent); margin-bottom: 0.5rem; font-size: 0.62rem; font-weight: 500; }
	.recipe dl { display: grid; grid-template-columns: auto 1fr; gap: 0.25rem 1rem; margin: 0; }
	.recipe dt { font: 500 0.6rem var(--f-mono); letter-spacing: 0.08em; text-transform: uppercase; color: var(--ink-3); padding-top: 0.1rem; }
	.recipe dd { margin: 0; font: 0.78rem var(--f-mono); }
	@media (max-width: 760px) {
		.panel { left: 0; top: auto; width: auto; height: 64%; padding: 1rem 16px 1.4rem; background: color-mix(in srgb, var(--ground) 96%, transparent); animation-name: enter-up; }
		@keyframes enter-up { from { opacity: 0; transform: translateY(40px); } }
	}
</style>
