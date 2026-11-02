import { authSession, type AppSession } from '../stores/auth';
import type { ApiResponse } from '../types';
import { wsManager } from '../ws';

import { get, post, put } from './http';

export const authApi = {
	async getSession(): Promise<ApiResponse<AppSession>> {
		const result = await get<AppSession>('/auth/session');
		authSession.set(result.data);
		if (!result.data.authenticated) wsManager.disconnect();
		return result;
	},

	async login(username: string, password: string) {
		await post('/auth/login', { username, password });
		return authApi.getSession();
	},

	async setup(setup_code: string, username: string, password: string) {
		await post('/auth/setup', { setup_code, username, password });
		return authApi.getSession();
	},

	async logout() {
		await post('/auth/logout');
		authSession.set({ setup_required: false, authenticated: false });
		wsManager.disconnect();
	},

	async changePassword(current_password: string, new_password: string) {
		await put('/auth/password', { current_password, new_password });
		wsManager.disconnect();
		return authApi.getSession();
	}
};
