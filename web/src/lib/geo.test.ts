import { expect, it } from 'vitest';
import { distanceKm, greatCircle, routeLine } from './geo';

const montreal: [number, number] = [-73.5674, 45.5019];
const lisbonne: [number, number] = [-9.13, 38.7114];

it('measures Montréal → Lisbonne at about 5 270 km', () => {
	expect(distanceKm(montreal, lisbonne)).toBeGreaterThan(5200);
	expect(distanceKm(montreal, lisbonne)).toBeLessThan(5350);
});

it('bends north between Montréal and Lisbonne', () => {
	const arc = greatCircle(montreal, lisbonne);
	expect(arc[0]).toEqual(montreal);
	expect(arc.at(-1)![0]).toBeCloseTo(lisbonne[0], 6);
	expect(Math.max(...arc.map((p) => p[1]))).toBeGreaterThan(47);
});

it('never jumps across the antimeridian', () => {
	const arc = greatCircle([170, 10], [-170, 12]);
	for (let i = 1; i < arc.length; i++) expect(Math.abs(arc[i][0] - arc[i - 1][0])).toBeLessThan(5);
});

it('chains legs without duplicating joints', () => {
	const line = routeLine([montreal, [-71.2075, 46.8123], [-69.716, 48.143]], 50);
	expect(line).toHaveLength(3);
	expect(routeLine([montreal])).toEqual([montreal]);
});
