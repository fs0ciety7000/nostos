// Server-side calls to nostos-api. The owner token never reaches the browser:
// it lives in an httpOnly cookie and is added here.
import { env } from '$env/dynamic/private';

export const SESSION_COOKIE = 'nostos_owner';

export function apiUrl(): string {
	return (env.NOSTOS_API_URL || 'http://127.0.0.1:8080').replace(/\/$/, '');
}

export class ApiUnavailable extends Error {}

export async function api(token: string | null, path: string, init: RequestInit = {}, fetcher: typeof fetch = fetch): Promise<Response> {
	const headers = new Headers(init.headers);
	if (token) headers.set('authorization', `Bearer ${token}`);
	try {
		return await fetcher(`${apiUrl()}${path}`, { ...init, headers });
	} catch (e) {
		throw new ApiUnavailable(`nostos-api is unreachable at ${apiUrl()}: ${(e as Error).message}`);
	}
}
