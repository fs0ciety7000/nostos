// MapLibre style for the Atlas: Protomaps vector tiles recoloured in the
// "Chambre noire" palette, a dotted land layer for the globe view, and the
// route and stop layers fed by the API.
import { layers, namedFlavor, type Flavor } from '@protomaps/basemaps';
import type { StyleSpecification, LayerSpecification, ExpressionSpecification } from 'maplibre-gl';

export const C = {
	ground: '#0b0709',
	earth: '#120a0e',
	dot: '#f2a27a',
	safelight: '#ff8a5c',
	hot: '#fff0dc',
	plan: '#7fc4d8',
	label: '#a58f93',
	pin: '#ffd2b0'
} as const;

/** Below this zoom the globe is drawn with dots; the basemap fades in above. */
export const DOTS_UNTIL = 5;

export const FLAVOR: Flavor = {
	...namedFlavor('black'),
	background: C.ground,
	earth: C.earth,
	water: C.ground,
	park_a: '#170e12', park_b: '#170e12', wood_a: '#180f13', wood_b: '#180f13', scrub_a: '#170e12', scrub_b: '#170e12',
	glacier: '#1a1216', sand: '#1a1114', beach: '#1d1316', industrial: '#130a0e', school: '#160d11', hospital: '#170e12',
	pedestrian: '#1c1216', aerodrome: '#1a1014', runway: '#2a1b21', zoo: '#170e12', military: '#140b0f', pier: '#1d1216',
	buildings: '#1f1418',
	other: '#24171d', minor_service: '#24171d', minor_a: '#2c1d24', minor_b: '#24171d', link: '#2c1d24',
	major: '#3a2630', highway: '#4a2f38', railway: '#33232a', boundaries: '#4d3540',
	minor_service_casing: C.earth, minor_casing: C.earth, link_casing: C.earth, major_casing_late: C.earth,
	highway_casing_late: C.earth, major_casing_early: C.earth, highway_casing_early: C.earth,
	roads_label_minor: '#6f5a60', roads_label_minor_halo: C.ground, roads_label_major: '#7d676c', roads_label_major_halo: C.ground,
	ocean_label: '#5a454c', subplace_label: '#7d676c', subplace_label_halo: C.ground, city_label: C.label,
	city_label_halo: C.ground, state_label: '#6f5a60', state_label_halo: C.ground, country_label: '#8f7a7f',
	address_label: '#6f5a60', address_label_halo: C.ground,
	regular: 'Noto Sans Regular', bold: 'Noto Sans Medium', italic: 'Noto Sans Italic'
};

const fadeIn: ExpressionSpecification = ['interpolate', ['linear'], ['zoom'], DOTS_UNTIL - 1.5, 0, DOTS_UNTIL, 1];

/** Fades a basemap layer in as the dotted globe fades out. */
function fade(layer: LayerSpecification): LayerSpecification {
	if (layer.type === 'background' || layer.id === 'earth' || layer.id === 'water') return layer;
	const key = layer.type === 'fill' ? 'fill-opacity' : layer.type === 'line' ? 'line-opacity' : layer.type === 'symbol' ? 'text-opacity' : null;
	if (!key) return layer;
	const paint = { ...(layer.paint as Record<string, unknown>) };
	// Only constant opacities can be wrapped; zoom-driven ones already ramp in.
	if (paint[key] === undefined || typeof paint[key] === 'number') {
		const v = (paint[key] as number | undefined) ?? 1;
		paint[key] = ['interpolate', ['linear'], ['zoom'], DOTS_UNTIL - 1.5, 0, DOTS_UNTIL, v];
	}
	return { ...layer, minzoom: Math.max(layer.minzoom ?? 0, DOTS_UNTIL - 1.5), paint } as LayerSpecification;
}

export interface StyleOptions {
	basemapUrl: string;
	glyphsUrl: string;
	lang?: string;
}

export function nostosStyle({ basemapUrl, glyphsUrl, lang = 'fr' }: StyleOptions): StyleSpecification {
	const base = layers('protomaps', FLAVOR, { lang }).map(fade);
	return {
		version: 8,
		projection: { type: 'globe' },
		glyphs: glyphsUrl,
		sky: {
			'sky-color': C.ground,
			'horizon-color': '#3a1620',
			'fog-color': C.ground,
			'sky-horizon-blend': 0.6,
			'horizon-fog-blend': 0.4,
			'fog-ground-blend': 0.9,
			// The physical (blue) atmosphere is replaced by the safelight halo drawn in Globe.svelte.
			'atmosphere-blend': 0
		},
		sources: {
			protomaps: { type: 'vector', url: `pmtiles://${basemapUrl}`, attribution: '© OpenStreetMap · Protomaps' },
			dots: { type: 'geojson', data: { type: 'FeatureCollection', features: [] } },
			route: { type: 'geojson', lineMetrics: true, data: { type: 'FeatureCollection', features: [] } },
			stops: { type: 'geojson', data: { type: 'FeatureCollection', features: [] }, promoteId: 'id' }
		},
		layers: [
			...base,
			{
				id: 'land-dots',
				type: 'circle',
				source: 'dots',
				// Past DOTS_UNTIL the dots stay as a faint texture: they keep land readable
				// beyond the edge of the regional basemap extract.
				maxzoom: 10,
				paint: {
					'circle-color': C.dot,
					'circle-radius': ['interpolate', ['exponential', 1.6], ['zoom'], 0, 0.9, 3, 1.8, 5, 2.4, 9, 2.6],
					'circle-opacity': ['interpolate', ['linear'], ['zoom'], 0, 0.85, DOTS_UNTIL - 1.2, 0.75, DOTS_UNTIL + 0.3, 0.28, 8.5, 0.2, 10, 0],
					'circle-blur': 0.25
				}
			},
			{
				id: 'route-glow',
				type: 'line',
				source: 'route',
				layout: { 'line-cap': 'round', 'line-join': 'round' },
				paint: { 'line-color': C.safelight, 'line-width': ['interpolate', ['linear'], ['zoom'], 1, 4, 8, 10], 'line-blur': 6, 'line-opacity': 0.35 }
			},
			{
				id: 'route',
				type: 'line',
				source: 'route',
				layout: { 'line-cap': 'round', 'line-join': 'round' },
				paint: { 'line-width': ['interpolate', ['linear'], ['zoom'], 1, 1.4, 8, 3], 'line-gradient': cometGradient(-1) }
			},
			{
				id: 'stops-halo',
				type: 'circle',
				source: 'stops',
				paint: {
					'circle-color': 'transparent',
					'circle-stroke-color': C.pin,
					'circle-stroke-width': 1,
					'circle-stroke-opacity': ['case', ['boolean', ['feature-state', 'selected'], false], 0.9, 0.35],
					'circle-radius': ['case', ['boolean', ['feature-state', 'selected'], false], 14, 8],
					'circle-pitch-alignment': 'map'
				}
			},
			{
				id: 'stops',
				type: 'circle',
				source: 'stops',
				paint: {
					'circle-color': ['case', ['boolean', ['feature-state', 'selected'], false], C.hot, ['get', 'waypoint'], C.ground, C.pin],
					'circle-stroke-color': C.pin,
					'circle-stroke-width': 1.5,
					'circle-radius': ['case', ['boolean', ['feature-state', 'selected'], false], 6, ['get', 'waypoint'], 3, 4.5],
					'circle-pitch-alignment': 'map'
				}
			}
		]
	};
}

/**
 * Route colour with a bright "comet" at `head` (0..1 along the line).
 * A negative head draws the plain route.
 */
export function cometGradient(head: number): ExpressionSpecification {
	const base = 'rgba(255,138,92,0.75)';
	if (head < 0 || head > 1) return ['interpolate', ['linear'], ['line-progress'], 0, base, 1, base];
	const tail = Math.max(0, head - 0.12);
	const stops: (number | string)[] = [];
	const push = (t: number, c: string) => {
		if (stops.length && (stops[stops.length - 2] as number) >= t) return;
		stops.push(t, c);
	};
	push(0, base);
	if (tail > 0) push(tail, base);
	push(Math.max(head, 0.0005), C.hot);
	if (head < 0.999) push(Math.min(1, head + 0.001), base);
	push(1, head >= 0.999 ? C.hot : base);
	return ['interpolate', ['linear'], ['line-progress'], ...stops] as ExpressionSpecification;
}
