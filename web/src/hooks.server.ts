import { redirect, type Handle } from '@sveltejs/kit';
import { SESSION_COOKIE } from '$lib/server/api';

const PUBLIC = ['/login'];

export const handle: Handle = async ({ event, resolve }) => {
	event.locals.ownerToken = event.cookies.get(SESSION_COOKIE) ?? null;
	const path = event.url.pathname;
	if (!event.locals.ownerToken && !PUBLIC.includes(path)) {
		if (path.startsWith('/api/')) return new Response(JSON.stringify({ error: 'unauthorized' }), { status: 401, headers: { 'content-type': 'application/json' } });
		redirect(303, '/login');
	}
	return resolve(event);
};
