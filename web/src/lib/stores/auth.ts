import { writable } from 'svelte/store';
export interface AppSession {
	setup_required: boolean;
	authenticated: boolean;
	username?: string;
	csrf_token?: string;
}
export const authSession = writable<AppSession | null>(null);
export function returnPath(value: string | null): string {
	if (!value?.startsWith('/')) return '/watch';
	try {
		const url = new URL(value, 'https://favkeep.invalid');
		return url.origin === 'https://favkeep.invalid' &&
			!/^\/(login|setup)([/?#]|$)/.test(url.pathname)
			? url.pathname + url.search + url.hash
			: '/watch';
	} catch {
		return '/watch';
	}
}
