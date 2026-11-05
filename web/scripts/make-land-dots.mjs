// Generates static/land-dots.json: evenly spread points (Fibonacci sphere)
// that fall on land, for the dotted globe shown at low zoom.
// Source: Natural Earth 1:110m land via world-atlas. Output: [[lon, lat], ...].
import { readFileSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { geoContains } from 'd3-geo';
import { feature } from 'topojson-client';

const require = createRequire(import.meta.url);
const topo = JSON.parse(readFileSync(require.resolve('world-atlas/land-110m.json'), 'utf8'));
const land = feature(topo, topo.objects.land);

const N = 60000;
const golden = Math.PI * (3 - Math.sqrt(5));
const out = [];
for (let i = 0; i < N; i++) {
	const y = 1 - (i / (N - 1)) * 2;
	const r = Math.sqrt(1 - y * y);
	const t = golden * i;
	const lat = (Math.asin(y) * 180) / Math.PI;
	const lon = (Math.atan2(Math.sin(t) * r, Math.cos(t) * r) * 180) / Math.PI;
	if (lat < -60) continue; // Antarctica would crowd the bottom of the globe
	if (geoContains(land, [lon, lat])) out.push([Math.round(lon * 100) / 100, Math.round(lat * 100) / 100]);
}
writeFileSync(new URL('../static/land-dots.json', import.meta.url), JSON.stringify(out));
console.log(`${out.length} land dots written to static/land-dots.json`);
