// Great-circle arcs, so long legs (Montréal → Lisbonne) curve on the globe.

const D = Math.PI / 180;

type LonLat = [number, number];

function toVec([lon, lat]: LonLat): [number, number, number] {
	return [Math.cos(lat * D) * Math.cos(lon * D), Math.cos(lat * D) * Math.sin(lon * D), Math.sin(lat * D)];
}

/** Angular distance in radians. */
export function angle(a: LonLat, b: LonLat): number {
	const [x1, y1, z1] = toVec(a);
	const [x2, y2, z2] = toVec(b);
	return Math.acos(Math.max(-1, Math.min(1, x1 * x2 + y1 * y2 + z1 * z2)));
}

/** Kilometres along the great circle. */
export function distanceKm(a: LonLat, b: LonLat): number {
	return angle(a, b) * 6371.0088;
}

/**
 * Points along the great circle from a to b. Longitudes are unwrapped so the
 * line never jumps across the antimeridian.
 */
export function greatCircle(a: LonLat, b: LonLat, step = 2): LonLat[] {
	const d = angle(a, b);
	const n = Math.max(1, Math.ceil(d / D / step));
	if (d < 1e-9) return [a, b];
	const A = toVec(a);
	const B = toVec(b);
	const out: LonLat[] = [];
	let prevLon = a[0];
	for (let i = 0; i <= n; i++) {
		const t = i / n;
		const s1 = Math.sin((1 - t) * d) / Math.sin(d);
		const s2 = Math.sin(t * d) / Math.sin(d);
		const [x, y, z] = [0, 1, 2].map((k) => A[k] * s1 + B[k] * s2);
		let lon = Math.atan2(y, x) / D;
		while (lon - prevLon > 180) lon -= 360;
		while (lon - prevLon < -180) lon += 360;
		prevLon = lon;
		out.push([lon, Math.asin(Math.max(-1, Math.min(1, z))) / D]);
	}
	return out;
}

/** A route through points, each leg densified along its great circle. */
export function routeLine(points: LonLat[], step = 2): LonLat[] {
	const out: LonLat[] = [];
	for (let i = 1; i < points.length; i++) {
		const leg = greatCircle(points[i - 1], points[i], step);
		out.push(...(i === 1 ? leg : leg.slice(1)));
	}
	return points.length === 1 ? [points[0]] : out;
}
