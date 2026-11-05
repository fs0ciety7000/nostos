import { expect, it } from 'vitest';
import { parseRange } from './range';

it('parses the ranges PMTiles clients send', () => {
	expect(parseRange('bytes=0-16383', 1000000)).toEqual({ start: 0, end: 16383 });
	expect(parseRange('bytes=999000-', 1000000)).toEqual({ start: 999000, end: 999999 });
	expect(parseRange('bytes=-100', 1000)).toEqual({ start: 900, end: 999 });
	expect(parseRange('bytes=500-2000', 1000)).toEqual({ start: 500, end: 999 });
});

it('refuses what it cannot or should not serve', () => {
	expect(parseRange(null, 1000)).toBeNull();
	expect(parseRange('bytes=1000-1001', 1000)).toBeNull();
	expect(parseRange('bytes=5-1', 1000)).toBeNull();
	expect(parseRange('bytes=0-1,5-9', 1000)).toBeNull();
	expect(parseRange(`bytes=0-${64 * 1024 * 1024}`, 1e9)).toBeNull();
});
