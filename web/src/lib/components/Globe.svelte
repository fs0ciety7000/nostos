<script lang="ts">
	// The Atlas globe: MapLibre in globe projection, dotted continents at low
	// zoom, Protomaps basemap when zoomed in, routes with a travelling glow,
	// and the camera flights between rest, trip and stop.
	import { onMount, untrack } from 'svelte';
	import maplibregl, { type Map as MlMap, type GeoJSONSource, type LngLatBoundsLike } from 'maplibre-gl';
	import 'maplibre-gl/dist/maplibre-gl.css';
	import { Protocol } from 'pmtiles';
	import { env } from '$env/dynamic/public';
	import { nostosStyle, cometGradient } from '$lib/map/style';
	import { angle, routeLine } from '$lib/geo';
	import type { Stop, TripDetail } from '$lib/types';

	interface Props {
		details: TripDetail[];
		tripSlug: string | null;
		stop: Stop | null;
		/** Screen space covered by panels, so flights centre in what's left. */
		padding: { left: number; right: number; bottom: number };
		onselecttrip: (slug: string) => void;
		onselectstop: (stop: Stop) => void;
		onmove?: (center: { lat: number; lon: number }) => void;
	}
	let { details, tripSlug, stop, padding, onselecttrip, onselectstop, onmove }: Props = $props();

	const REST = { center: [-38, 32] as [number, number], zoom: 1.55 };
	const reduce = typeof matchMedia !== 'undefined' && matchMedia('(prefers-reduced-motion: reduce)').matches;

	let el: HTMLDivElement;
	let halo: HTMLDivElement;
	let map: MlMap | null = $state(null);
	let ready = $state(false);
	let failed = $state<string | null>(null);
	let labels: { el: HTMLButtonElement; lngLat: [number, number]; kind: 'trip' | 'stop'; trip: string; id: string }[] = [];

	let protocolAdded = false;

	onMount(() => {
		if (!protocolAdded) {
			const protocol = new Protocol();
			maplibregl.addProtocol('pmtiles', protocol.tile);
			protocolAdded = true;
		}
		let m: MlMap;
		try {
			m = new maplibregl.Map({
				container: el,
				style: nostosStyle({
					basemapUrl: env.PUBLIC_BASEMAP_URL || new URL('/basemap.pmtiles', location.href).href,
					glyphsUrl: env.PUBLIC_GLYPHS_URL || 'https://protomaps.github.io/basemaps-assets/fonts/{fontstack}/{range}.pbf'
				}),
				center: REST.center,
				zoom: REST.zoom,
				attributionControl: { compact: true },
				maxPitch: 60,
				fadeDuration: 200
			});
		} catch (e) {
			failed = 'Le globe ne peut pas s’afficher : WebGL est indisponible dans ce navigateur.';
			return;
		}
		map = m;
		m.on('error', (e) => console.warn('map:', e.error?.message ?? e));
		m.on('load', async () => {
			ready = true;
			try {
				const pts: [number, number][] = await (await fetch('/land-dots.json')).json();
				(m.getSource('dots') as GeoJSONSource).setData({
					type: 'FeatureCollection',
					features: [{ type: 'Feature', properties: {}, geometry: { type: 'MultiPoint', coordinates: pts } }]
				});
			} catch {
				/* the basemap still shows land */
			}
			pushData();
			placeHalo();
			fly();
		});
		m.on('move', () => {
			const c = m.getCenter();
			onmove?.({ lat: c.lat, lon: c.lng });
			placeLabels();
			placeHalo();
		});
		m.on('click', 'stops', (e) => {
			const id = e.features?.[0]?.properties?.id as string | undefined;
			const s = details.flatMap((d) => d.stops).find((x) => x.id === id);
			if (s) onselectstop(s);
		});
		m.on('mouseenter', 'stops', () => (m.getCanvas().style.cursor = 'pointer'));
		m.on('mouseleave', 'stops', () => (m.getCanvas().style.cursor = ''));
		for (const ev of ['mousedown', 'touchstart', 'wheel'] as const) m.on(ev, () => (idleSince = performance.now()));

		let raf = 0;
		let last = performance.now();
		let idleSince = performance.now();
		let frame = 0;
		const loop = (t: number) => {
			raf = requestAnimationFrame(loop);
			const dt = Math.min(0.05, (t - last) / 1000);
			last = t;
			if (!ready || document.hidden) return;
			// Slow spin at rest, after a few seconds without interaction.
			if (!reduce && !tripSlug && !m.isMoving() && t - idleSince > 3500) {
				const c = m.getCenter();
				m.jumpTo({ center: [c.lng + dt * 4, c.lat] });
			}
			if (!reduce && ++frame % 2 === 0) m.setPaintProperty('route', 'line-gradient', cometGradient(((t / 1000) * 0.14) % 1.25));
		};
		raf = requestAnimationFrame(loop);
		return () => {
			cancelAnimationFrame(raf);
			labels.forEach((l) => l.el.remove());
			m.remove();
		};
	});

	function pushData() {
		if (!map || !ready) return;
		const m = map;
		(m.getSource('route') as GeoJSONSource).setData({
			type: 'FeatureCollection',
			features: details
				.filter((d) => d.stops.length > 1)
				.map((d) => ({ type: 'Feature', properties: { trip: d.trip.slug }, geometry: { type: 'LineString', coordinates: routeLine(d.stops.map((s) => [s.lon, s.lat])) } }))
		});
		(m.getSource('stops') as GeoJSONSource).setData({
			type: 'FeatureCollection',
			features: details.flatMap((d) =>
				d.stops.map((s) => ({ type: 'Feature' as const, properties: { id: s.id, trip: d.trip.slug, waypoint: s.is_waypoint }, geometry: { type: 'Point' as const, coordinates: [s.lon, s.lat] } }))
			)
		});
		labels.forEach((l) => l.el.remove());
		labels = [];
		const layer = el.parentElement?.querySelector('.labels');
		for (const d of details) {
			if (!d.stops.length) continue;
			const first = d.stops[0];
			labels.push(makeLabel('trip', d.trip.slug, d.trip.slug, [first.lon, first.lat], `${d.trip.title}<small>${d.stops.length} escales</small>`, layer));
			for (const s of d.stops) {
				if (s.is_waypoint) continue;
				labels.push(makeLabel('stop', d.trip.slug, s.id, [s.lon, s.lat], escapeHtml(s.name ?? `Escale ${s.seq}`), layer));
			}
		}
		placeLabels();
	}

	function escapeHtml(s: string) {
		return s.replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]!);
	}

	function makeLabel(kind: 'trip' | 'stop', trip: string, id: string, lngLat: [number, number], html: string, layer: Element | null | undefined) {
		const b = document.createElement('button');
		b.type = 'button';
		b.className = `pin ${kind}`;
		b.dataset.off = '1';
		b.dataset.id = id;
		b.innerHTML = html;
		b.addEventListener('click', () => {
			if (kind === 'trip') onselecttrip(trip);
			else {
				const s = details.find((d) => d.trip.slug === trip)?.stops.find((x) => x.id === id);
				if (s) onselectstop(s);
			}
		});
		(layer ?? el).appendChild(b);
		return { el: b, lngLat, kind, trip, id };
	}

	/** Shows labels on the visible side of the globe, without overlaps. */
	function placeLabels() {
		if (!map) return;
		const m = map;
		const { width, height } = m.getCanvas().getBoundingClientRect();
		const placed: [number, number, number][] = [];
		const order = [...labels].sort((a, b) => Number(b.id === stop?.id) - Number(a.id === stop?.id));
		for (const l of order) {
			let show = l.kind === 'trip' ? !tripSlug : l.trip === tripSlug;
			const p = m.project(l.lngLat);
			// Hide what is behind the globe (more than ~75° from the view centre).
			const c = m.getCenter();
			const hidden = m.getZoom() < 5 && angle([c.lng, c.lat], l.lngLat) > (75 * Math.PI) / 180;
			if (hidden || p.x < 0 || p.y < 0 || p.x > width || p.y > height) show = false;
			const w = l.el.offsetWidth || 80;
			if (show && p.x + 10 + w > width - 8) show = false;
			if (show && placed.some(([x, y, pw]) => p.x < x + pw + 8 && x < p.x + w + 8 && Math.abs(y - p.y) < 22)) show = false;
			if (show) placed.push([p.x, p.y, w]);
			l.el.dataset.off = show ? '0' : '1';
			l.el.classList.toggle('sel', l.id === stop?.id);
			if (show) l.el.style.transform = `translate3d(${(p.x + 8).toFixed(1)}px, ${(p.y - 11).toFixed(1)}px, 0)`;
		}
	}

	/** Safelight glow around the globe's limb, fading out as the camera nears the ground. */
	function placeHalo() {
		if (!map || !halo) return;
		const z = map.getZoom();
		// Globe radius in CSS pixels: world size (512 · 2^z) over 2π.
		const r = ((512 * 2 ** z) / (2 * Math.PI)) * 0.985;
		const p = map.project(map.getCenter());
		halo.style.transform = `translate3d(${p.x - r * 1.35}px, ${p.y - r * 1.35}px, 0)`;
		halo.style.width = halo.style.height = `${r * 2.7}px`;
		halo.style.opacity = String(Math.max(0, Math.min(1, (3.6 - z) / 1.4)));
	}

	function fly() {
		if (!map || !ready) return;
		const m = map;
		const opts = { padding: { top: 40, left: padding.left + 24, right: padding.right + 24, bottom: padding.bottom + 40 }, duration: reduce ? 0 : 1800, essential: true };
		for (const d of details) for (const s of d.stops) m.setFeatureState({ source: 'stops', id: s.id }, { selected: s.id === stop?.id });
		if (stop) {
			m.flyTo({ ...opts, center: [stop.lon, stop.lat], zoom: 12.5, pitch: 0, curve: 1.6 });
		} else if (tripSlug) {
			const d = details.find((x) => x.trip.slug === tripSlug);
			if (!d?.stops.length) return;
			const lons = d.stops.map((s) => s.lon);
			const lats = d.stops.map((s) => s.lat);
			const bounds: LngLatBoundsLike = [
				[Math.min(...lons), Math.min(...lats)],
				[Math.max(...lons), Math.max(...lats)]
			];
			m.fitBounds(bounds, { ...opts, maxZoom: 8.5, linear: false });
		} else {
			m.flyTo({ ...opts, padding: { top: 0, bottom: 0, left: padding.left, right: 0 }, center: REST.center, zoom: REST.zoom, curve: 1.4 });
		}
	}

	$effect(() => {
		void details;
		untrack(pushData);
	});
	$effect(() => {
		void tripSlug;
		void stop;
		void padding.left;
		void padding.right;
		void padding.bottom;
		untrack(() => {
			fly();
			placeLabels();
		});
	});
</script>

<div class="halo" bind:this={halo} aria-hidden="true"></div>
<div class="globe" bind:this={el} role="application" aria-label="Globe des voyages : glisse pour tourner, utilise la liste pour choisir une escale"></div>
{#if failed}<p class="failed" role="alert">{failed}</p>{/if}

<style>
	.globe { position: absolute; inset: 0; }
	.halo {
		position: absolute; left: 0; top: 0; z-index: 1; pointer-events: none; opacity: 0; will-change: transform;
		/* The ring sits on the limb (radius 1/1.35 of the box): bright edge, long soft falloff. */
		background: radial-gradient(circle closest-side, transparent 72.6%, rgba(255, 106, 61, 0.55) 74%, rgba(255, 90, 50, 0.22) 78%, rgba(255, 80, 40, 0.07) 88%, transparent 100%);
		mix-blend-mode: screen;
	}
	.globe :global(.maplibregl-canvas) { outline: none; }
	.globe :global(.maplibregl-ctrl-attrib) { background: rgba(11, 7, 9, 0.7); color: var(--ink-3); font: 500 0.6rem var(--f-mono); }
	.globe :global(.maplibregl-ctrl-attrib a) { color: var(--ink-2); }
	.globe :global(.maplibregl-ctrl-attrib-button) { filter: invert(0.8); }
	.failed { position: absolute; inset: auto var(--gut) 50% auto; max-width: 30ch; color: var(--ink-2); }
</style>
