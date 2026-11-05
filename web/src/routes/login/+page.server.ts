import { fail, redirect } from '@sveltejs/kit';
import { api, ApiUnavailable, SESSION_COOKIE } from '$lib/server/api';
import type { Actions } from './$types';

export const actions: Actions = {
	default: async ({ request, cookies, url, fetch }) => {
		const token = String((await request.formData()).get('token') ?? '').trim();
		if (!token) return fail(400, { message: 'Colle le jeton propriétaire défini dans NOSTOS_OWNER_TOKEN.' });
		let res: Response;
		try {
			res = await api(token, '/trips', {}, fetch);
		} catch (e) {
			if (e instanceof ApiUnavailable) return fail(503, { message: "L'API Nostos ne répond pas. Vérifie qu'elle tourne et que NOSTOS_API_URL pointe dessus." });
			throw e;
		}
		if (res.status === 401) return fail(401, { message: 'Ce jeton est refusé par l’API. Vérifie NOSTOS_OWNER_TOKEN.' });
		if (!res.ok) return fail(502, { message: `L'API a répondu ${res.status}.` });
		cookies.set(SESSION_COOKIE, token, {
			path: '/',
			httpOnly: true,
			sameSite: 'strict',
			secure: url.protocol === 'https:',
			maxAge: 60 * 60 * 24 * 30
		});
		redirect(303, '/');
	}
};
