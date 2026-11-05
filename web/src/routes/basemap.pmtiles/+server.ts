// Development relay for the basemap. In production PUBLIC_BASEMAP_URL points
// at the PMTiles extract on R2 (served with CORS) and this route is unused.
// Here, range requests are forwarded to BASEMAP_UPSTREAM (default: the
// Protomaps daily planet build, which browsers cannot read directly).
import { error } from '@sveltejs/kit';
import { env } from '$env/dynamic/private';

const FORWARD = ['range', 'if-match', 'if-none-match'];
const RETURN = ['content-range', 'content-length', 'content-type', 'etag', 'accept-ranges', 'last-modified'];

export async function GET({ request, fetch }) {
	const upstream = env.BASEMAP_UPSTREAM;
	if (!upstream) error(404, 'Set PUBLIC_BASEMAP_URL to the basemap on R2, or BASEMAP_UPSTREAM to relay one in development.');
	const headers = new Headers();
	for (const h of FORWARD) {
		const v = request.headers.get(h);
		if (v) headers.set(h, v);
	}
	if (!headers.has('range')) error(416, 'PMTiles are read with range requests only.');
	const res = await fetch(upstream, { headers });
	const out = new Headers({ 'cache-control': 'private, max-age=86400' });
	for (const h of RETURN) {
		const v = res.headers.get(h);
		if (v) out.set(h, v);
	}
	return new Response(res.body, { status: res.status, headers: out });
}
