import type {
	ApiResponse,
	ContinueWatchingItem,
	WatchProgress,
	WatchProgressUpdate
} from '../types';

import { get, request } from './http';

export const watchApi = {
	async getContinueWatching(): Promise<ApiResponse<ContinueWatchingItem[]>> {
		return get<ContinueWatchingItem[]>('/continue-watching');
	},

	async getWatchProgress(id: number): Promise<ApiResponse<WatchProgress | null>> {
		return get<WatchProgress | null>(`/watch-progress/${id}`);
	},

	async saveWatchProgress(
		id: number,
		data: WatchProgressUpdate,
		keepalive = false
	): Promise<ApiResponse<WatchProgress>> {
		return request<WatchProgress>(`/watch-progress/${id}`, 'PUT', data, undefined, keepalive);
	}
};
