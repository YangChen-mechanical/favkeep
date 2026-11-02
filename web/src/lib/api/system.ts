import type { ApiResponse, DashBoardResponse, SysInfo, TaskStatus } from '../types';
import { wsManager } from '../ws';

import { get, post } from './http';

export const systemApi = {
	async getDashboard(): Promise<ApiResponse<DashBoardResponse>> {
		return get<DashBoardResponse>('/dashboard');
	},

	async triggerDownloadTask(): Promise<ApiResponse<boolean>> {
		return post<boolean>('/task/download');
	},

	subscribeToLogs(onMessage: (data: string) => void) {
		return wsManager.subscribeToLogs(onMessage);
	},

	subscribeToSysInfo(onMessage: (data: SysInfo) => void) {
		return wsManager.subscribeToSysInfo(onMessage);
	},

	subscribeToTasks(onMessage: (data: TaskStatus) => void) {
		return wsManager.subscribeToTasks(onMessage);
	}
};
