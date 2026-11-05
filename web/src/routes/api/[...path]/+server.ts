// Read-only proxy to nostos-api for the browser, adding the owner token.
// Only the routes the Atlas needs are forwarded.
import { error } from '@sveltejs/kit';
import { api, ApiUnavailable } from '$lib/server/api';

const ALLOWED = [/^trips$/, /^trips\/[a-z0-9-]+$/, /^trips\/[a-z0-9-]+\/(photos|route)$/, /^photos\/[0-9a-f-]{36}$/];

export async function GET({ params, locals, fetch }) {
	if (!ALLOWED.some((re) => re.test(params.path))) error(404, 'Not found');
	try {
		const res = await api(locals.ownerToken, `/${params.path}`, {}, fetch);
		return new Response(res.body, { status: res.status, headers: { 'content-type': res.headers.get('content-type') ?? 'application/json', 'cache-control': 'private, no-store' } });
	} catch (e) {
		if (e instanceof ApiUnavailable) error(503, "L'API Nostos ne répond pas.");
		throw e;
	}
}
