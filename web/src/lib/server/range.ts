/** PMTiles clients read directories and tile runs; nothing legitimate asks for more. */
const MAX_RANGE = 32 * 1024 * 1024;

/** Parses a single `bytes=a-b` range (the only form PMTiles clients send). */
export function parseRange(header: string | null, size: number): { start: number; end: number } | null {
	const m = header ? /^bytes=(\d*)-(\d*)$/.exec(header.trim()) : null;
	if (!m || (m[1] === '' && m[2] === '')) return null;
	let start: number, end: number;
	if (m[1] === '') {
		start = Math.max(0, size - Number(m[2]));
		end = size - 1;
	} else {
		start = Number(m[1]);
		end = m[2] === '' ? size - 1 : Math.min(Number(m[2]), size - 1);
	}
	if (start > end || start >= size || end - start + 1 > MAX_RANGE) return null;
	return { start, end };
}
