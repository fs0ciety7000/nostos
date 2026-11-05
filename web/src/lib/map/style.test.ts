import { expect, it } from 'vitest';
import { cometGradient, nostosStyle } from './style';

it('builds a globe style with the dotted land and data layers on top', () => {
	const s = nostosStyle({ basemapUrl: 'https://example.com/basemap.pmtiles', glyphsUrl: 'https://example.com/{fontstack}/{range}.pbf' });
	expect(s.projection).toEqual({ type: 'globe' });
	expect((s.sources.protomaps as { url: string }).url).toBe('pmtiles://https://example.com/basemap.pmtiles');
	const ids = s.layers.map((l) => l.id);
	expect(ids.slice(-5)).toEqual(['land-dots', 'route-glow', 'route', 'stops-halo', 'stops']);
	expect(ids).toContain('earth');
});

it('keeps comet gradient stops strictly increasing at every head position', () => {
	for (let h = -0.1; h <= 1.05; h += 0.013) {
		const e = cometGradient(h) as unknown[];
		const ts = e.slice(3).filter((_, i) => i % 2 === 0) as number[];
		expect(ts[0]).toBe(0);
		expect(ts.at(-1)).toBe(1);
		for (let i = 1; i < ts.length; i++) expect(ts[i]).toBeGreaterThan(ts[i - 1]);
	}
});
