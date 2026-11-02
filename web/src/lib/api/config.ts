import type { ApiResponse, Config, Credential, Notifier, NotifiersResponse } from '../types';

import { get, post, put, request } from './http';

export const configApi = {
	async getNotifiers(): Promise<ApiResponse<NotifiersResponse>> {
		return get('/config/notifiers');
	},

	async saveNotifier(
		version: number,
		notifier: Notifier,
		index?: number
	): Promise<ApiResponse<NotifiersResponse>> {
		return request(
			'/config/notifiers' + (index === undefined ? '' : '/' + index),
			index === undefined ? 'POST' : 'PUT',
			{ version, notifier }
		);
	},

	async deleteNotifier(version: number, index: number): Promise<ApiResponse<NotifiersResponse>> {
		return request('/config/notifiers/' + index, 'DELETE', { version });
	},

	async testNotifier(version: number, index: number) {
		return post('/config/notifiers/ping', { version, index });
	},

	async updateCredential(version: number, credential: Credential): Promise<ApiResponse<Config>> {
		return put('/config/credential', { version, credential });
	},

	async getConfig(): Promise<ApiResponse<Config>> {
		return get<Config>('/config');
	},

	async updateConfig(config: Config): Promise<ApiResponse<Config>> {
		return put<Config>('/config', config);
	}
};
