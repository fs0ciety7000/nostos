declare global {
	namespace App {
		interface Locals {
			/** Owner token from the session cookie, forwarded to the API. */
			ownerToken: string | null;
		}
	}
}
export {};
