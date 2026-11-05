// Development and self-hosting fallback for the basemap. In production
// PUBLIC_BASEMAP_URL points at the PMTiles extract on R2 (served with CORS)
// and this route is unused. Otherwise it serves, in order of preference:
//   - BASEMAP_FILE: a local extract, e.g. ../tiles/nostos-basemap.pmtiles;
//   - BASEMAP_UPSTREAM: a remote archive relayed with range requests (the
//     Protomaps daily planet, which browsers cannot read directly).
import { error } from '@sveltejs/kit';
import { env } from '$env/dynamic/private';
import { open, stat } from 'node:fs/promises';
import { parseRange } from '$lib/server/range';

const FORWARD = ['range', 'if-match', 'if-none-match'];
const RETURN = ['content-range', 'content-length', 'content-type', 'etag', 'accept-ranges', 'last-modified'];
async function fromFile(path: string, request: Request): Promise<Response> {
	const info = await stat(path).catch(() => null);
	if (!info?.isFile()) error(404, `BASEMAP_FILE ${path} does not exist`);
	const etag = `"${info.size.toString(16)}-${Math.floor(info.mtimeMs).toString(16)}"`;
	const ifMatch = request.headers.get('if-match');
	if (ifMatch && ifMatch !== etag) return new Response(null, { status: 412, headers: { etag } });
	const range = parseRange(request.headers.get('range'), info.size);
	if (!range) return new Response(null, { status: 416, headers: { 'content-range': `bytes */${info.size}` } });
	const len = range.end - range.start + 1;
	const buf = Buffer.alloc(len);
	const fh = await open(path, 'r');
	try {
		await fh.read(buf, 0, len, range.start);
	} finally {
		await fh.close();
	}
	return new Response(buf, {
		status: 206,
		headers: {
			'content-type': 'application/octet-stream',
			'content-range': `bytes ${range.start}-${range.end}/${info.size}`,
			'content-length': String(len),
			'accept-ranges': 'bytes',
			etag,
			'cache-control': 'private, max-age=86400'
		}
	});
}

export async function GET({ request, fetch }) {
	if (env.BASEMAP_FILE) return fromFile(env.BASEMAP_FILE, request);
	const upstream = env.BASEMAP_UPSTREAM;
	if (!upstream) error(404, 'Set PUBLIC_BASEMAP_URL to the basemap on R2, or BASEMAP_FILE / BASEMAP_UPSTREAM to serve one from here.');
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
