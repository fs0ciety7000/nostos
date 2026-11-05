// "Philtres": how a film simulation colours the gallery, and how a recipe
// reads in the camera's own terms.
import type { Recipe } from './types';

export interface Tone {
	/** Accent used for rules, the selected chip and the chant numeral. */
	accent: string;
	/** Very dark tint behind the gallery, keeps photos the brightest thing. */
	ground: string;
}

/** Hand-tuned tones per simulation, used when a stop has no palette yet. */
const TONES: Record<string, Tone> = {
	'Classic Chrome': { accent: '#c9a77c', ground: '#11100e' },
	'Classic Neg.': { accent: '#d98f6a', ground: '#130d0c' },
	'Nostalgic Neg.': { accent: '#f2b36a', ground: '#140d09' },
	Velvia: { accent: '#ff7a59', ground: '#140909' },
	Provia: { accent: '#9fc1d9', ground: '#0b0e12' },
	Astia: { accent: '#e8b3b8', ground: '#120c0e' },
	Eterna: { accent: '#9db5a6', ground: '#0c100e' },
	'Eterna Bleach Bypass': { accent: '#b8b0a0', ground: '#0f0e0c' },
	'Reala Ace': { accent: '#e6c48f', ground: '#120f0b' },
	'Pro Neg. Hi': { accent: '#d8a48c', ground: '#120d0b' },
	'Pro Neg. Std': { accent: '#c9b2a4', ground: '#100d0c' }
};
const SAFELIGHT: Tone = { accent: '#ff8a5c', ground: '#0b0709' };
const MONO: Tone = { accent: '#d9d4cf', ground: '#0c0c0c' };

export function isMono(film: string | null | undefined): boolean {
	return !!film && /^(Acros|Monochrome|Sepia)/.test(film);
}

function hexToRgb(hex: string): [number, number, number] | null {
	const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
	if (!m) return null;
	const n = parseInt(m[1], 16);
	return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

function toHex([r, g, b]: [number, number, number]): string {
	return '#' + [r, g, b].map((v) => Math.round(Math.max(0, Math.min(255, v))).toString(16).padStart(2, '0')).join('');
}

/** Relative luminance (WCAG). */
export function luminance(hex: string): number {
	const rgb = hexToRgb(hex);
	if (!rgb) return 0;
	const [r, g, b] = rgb.map((c) => {
		const s = c / 255;
		return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
	});
	return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** Contrast ratio between two colours (1..21). */
export function contrast(a: string, b: string): number {
	const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
	return (hi + 0.05) / (lo + 0.05);
}

/**
 * Tone for a stop: the most saturated colour of its photos' palette, lifted
 * until it reads on the dark ground (contrast ≥ 4.5), else the simulation's
 * own tone, else the darkroom safelight.
 */
export function toneFor(film: string | null | undefined, palette?: string[] | null): Tone {
	const base = isMono(film) ? MONO : (film && TONES[film]) || SAFELIGHT;
	if (isMono(film) || !palette?.length) return base;
	const scored = palette
		.map((h) => ({ h, rgb: hexToRgb(h) }))
		.filter((c): c is { h: string; rgb: [number, number, number] } => !!c.rgb)
		.map((c) => ({ ...c, sat: Math.max(...c.rgb) - Math.min(...c.rgb) }))
		.sort((a, b) => b.sat - a.sat);
	const pick = scored[0];
	if (!pick || pick.sat < 40) return base;
	let rgb = pick.rgb;
	for (let i = 0; i < 12 && contrast(toHex(rgb), base.ground) < 4.5; i++) {
		rgb = rgb.map((c) => c + (255 - c) * 0.18) as [number, number, number];
	}
	return { accent: toHex(rgb), ground: base.ground };
}

const STRENGTH: Record<string, string> = { Off: 'Off', Weak: 'Faible', Strong: 'Fort' };
const SIZE: Record<string, string> = { Off: '', Small: 'petit', Large: 'grand' };

function signed(v: number | null | undefined): string | null {
	if (v === null || v === undefined) return null;
	const s = Number.isInteger(v) ? String(Math.abs(v)) : Math.abs(v).toFixed(1).replace('.', ',');
	return v === 0 ? '0' : (v > 0 ? '+' : '−') + s;
}

/** The recipe as label/value rows, in the camera's menu wording. */
export function recipeRows(r: Recipe | null | undefined): [string, string][] {
	if (!r) return [];
	const rows: [string, string | null][] = [];
	if (r.grain_roughness) {
		const size = r.grain_size ? SIZE[r.grain_size] : '';
		rows.push(['Grain', r.grain_roughness === 'Off' ? 'Off' : [STRENGTH[r.grain_roughness], size].filter(Boolean).join(' · ')]);
	}
	if (r.color_chrome) rows.push(['Color Chrome', STRENGTH[r.color_chrome]]);
	if (r.color_chrome_fx_blue) rows.push(['CC bleu', STRENGTH[r.color_chrome_fx_blue]]);
	rows.push(['Ombres', signed(r.shadow_tone)]);
	rows.push(['Hautes lum.', signed(r.highlight_tone)]);
	rows.push(['Couleur', signed(r.color)]);
	rows.push(['Clarté', signed(r.clarity)]);
	if (r.wb_shift) {
		const { red_raw, blue_raw } = r.wb_shift;
		const scale = red_raw % 20 === 0 && blue_raw % 20 === 0 ? 20 : 1;
		rows.push(['Bal. blancs', `R${signed(red_raw / scale)} B${signed(blue_raw / scale)}`]);
	}
	if (r.dynamic_range) rows.push(['Plage dyn.', r.dynamic_range === 'Auto' ? 'Auto' : `DR${r.dynamic_range.Percent}`]);
	return rows.filter((row): row is [string, string] => row[1] !== null);
}

/** "f/5.6 · 1/250 · ISO 160 · 23 mm" */
export function exposureLine(p: { f_number: number | null; exposure_time: string | null; iso: number | null; focal_mm: number | null }): string {
	return [
		p.f_number ? `f/${String(Math.round(p.f_number * 10) / 10).replace('.', ',')}` : null,
		p.exposure_time,
		p.iso ? `ISO ${p.iso}` : null,
		p.focal_mm ? `${Math.round(p.focal_mm)} mm` : null
	]
		.filter(Boolean)
		.join(' · ');
}
