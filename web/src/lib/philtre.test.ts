import { describe, expect, it } from 'vitest';
import { contrast, exposureLine, recipeRows, toneFor } from './philtre';

describe('toneFor', () => {
	it('falls back to the simulation tone without a palette', () => {
		expect(toneFor('Classic Chrome').accent).toBe('#c9a77c');
		expect(toneFor(null).accent).toBe('#ff8a5c');
	});
	it('keeps monochrome neutral whatever the palette', () => {
		expect(toneFor('Acros+R', ['#ff0000']).accent).toBe('#d9d4cf');
	});
	it('picks the most saturated palette colour and makes it readable', () => {
		const t = toneFor('Velvia', ['#202020', '#3a1d10', '#8a2a14']);
		expect(contrast(t.accent, t.ground)).toBeGreaterThanOrEqual(4.5);
		const [r, g, b] = [1, 3, 5].map((i) => parseInt(t.accent.slice(i, i + 2), 16));
		expect(r).toBeGreaterThan(g);
		expect(r).toBeGreaterThan(b);
	});
	it('ignores a palette of greys', () => {
		expect(toneFor('Eterna', ['#333333', '#777777']).accent).toBe('#9db5a6');
	});
});

describe('recipeRows', () => {
	it('reads like the camera menu', () => {
		const rows = Object.fromEntries(
			recipeRows({
				grain_roughness: 'Weak',
				grain_size: 'Small',
				color_chrome: 'Strong',
				shadow_tone: 1,
				highlight_tone: -0.5,
				color: 0,
				wb_shift: { red_raw: 40, blue_raw: -80 },
				dynamic_range: { Percent: 400 }
			})
		);
		expect(rows).toMatchObject({
			Grain: 'Faible · petit',
			'Color Chrome': 'Fort',
			Ombres: '+1',
			'Hautes lum.': '−0,5',
			Couleur: '0',
			'Bal. blancs': 'R+2 B−4',
			'Plage dyn.': 'DR400'
		});
		expect(rows).not.toHaveProperty('Clarté');
	});
	it('is empty without a recipe', () => {
		expect(recipeRows(null)).toEqual([]);
	});
});

it('formats the exposure line', () => {
	expect(exposureLine({ f_number: 5.6, exposure_time: '1/250', iso: 160, focal_mm: 23 })).toBe('f/5,6 · 1/250 · ISO 160 · 23 mm');
	expect(exposureLine({ f_number: null, exposure_time: null, iso: null, focal_mm: null })).toBe('');
});
