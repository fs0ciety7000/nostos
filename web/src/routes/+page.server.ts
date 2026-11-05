import { redirect } from '@sveltejs/kit';
import { api, ApiUnavailable, SESSION_COOKIE } from '$lib/server/api';
import type { TripDetail, TripSummary } from '$lib/types';

export async function load({ locals, cookies, fetch }) {
	const empty = { trips: [] as TripSummary[], details: [] as TripDetail[] };
	try {
		const res = await api(locals.ownerToken, '/trips', {}, fetch);
		if (res.status === 401) {
			cookies.delete(SESSION_COOKIE, { path: '/' });
			redirect(303, '/login');
		}
		if (!res.ok) return { ...empty, apiError: `L'API a répondu ${res.status}.` };
		const trips = (await res.json()) as TripSummary[];
		// A handful of trips: their stops are needed to place them on the globe.
		const details = await Promise.all(
			trips.map(async (t) => {
				const r = await api(locals.ownerToken, `/trips/${t.slug}`, {}, fetch);
				return r.ok ? ((await r.json()) as TripDetail) : null;
			})
		);
		return { trips, details: details.filter((d): d is TripDetail => d !== null), apiError: null };
	} catch (e) {
		if (e instanceof ApiUnavailable) return { ...empty, apiError: "L'API Nostos ne répond pas. Les voyages réapparaîtront dès qu'elle sera relancée." };
		throw e;
	}
}
