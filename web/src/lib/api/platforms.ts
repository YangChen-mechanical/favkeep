import type {
	ApiResponse,
	DouyinFolder,
	DouyinFoldersResponse,
	DouyinStatus,
	QrcodeGenerateResponse as GenerateQrcodeResponse,
	Platform,
	PlatformInfo,
	QrcodePollResponse as PollQrcodeResponse,
	XiaohongshuSource,
	XiaohongshuStatus
} from '../types';

import { get, post, put } from './http';

export const platformsApi = {
	getPlatforms(): Promise<ApiResponse<PlatformInfo[]>> {
		return get('/platforms');
	},

	async getDouyinStatus(): Promise<ApiResponse<DouyinStatus>> {
		return get('/douyin/status');
	},

	async updateDouyinConfig(interval: number): Promise<ApiResponse<DouyinStatus>> {
		return put('/config/douyin', { interval });
	},

	async getDouyinFolders(): Promise<ApiResponse<DouyinFoldersResponse>> {
		return get('/video-sources/douyin');
	},

	async refreshDouyinFolders(): Promise<ApiResponse<DouyinFoldersResponse>> {
		return post('/video-sources/douyin/refresh');
	},

	async updateDouyinFolder(id: string, enabled: boolean): Promise<ApiResponse<DouyinFolder>> {
		return put(`/video-sources/douyin/${encodeURIComponent(id)}`, { enabled });
	},

	async triggerDouyinTask(): Promise<ApiResponse<boolean>> {
		return post('/task/douyin');
	},

	async resumeDouyin(): Promise<ApiResponse<unknown>> {
		return post('/douyin/resume');
	},

	async getXiaohongshuStatus(): Promise<ApiResponse<XiaohongshuStatus>> {
		return get('/xiaohongshu/status');
	},

	async updateXiaohongshuConfig(interval: number): Promise<ApiResponse<XiaohongshuStatus>> {
		return put('/config/xiaohongshu', { interval });
	},

	async getXiaohongshuSource(): Promise<ApiResponse<XiaohongshuSource>> {
		return get('/video-sources/xiaohongshu');
	},

	async updateXiaohongshuSource(enabled: boolean): Promise<ApiResponse<XiaohongshuSource>> {
		return put('/video-sources/xiaohongshu', { enabled });
	},

	async triggerXiaohongshuTask(): Promise<ApiResponse<boolean>> {
		return post('/task/xiaohongshu');
	},

	async resumeXiaohongshu(): Promise<ApiResponse<unknown>> {
		return post('/xiaohongshu/resume');
	},

	async cancelQrcode(qrcodeKey: string, platform: Platform): Promise<ApiResponse<unknown>> {
		return post('/login/qrcode/poll', { qrcode_key: qrcodeKey, platform, cancel: true });
	},

	async generateQrcode(
		platform: Platform = 'bilibili',
		forceQr = false
	): Promise<ApiResponse<GenerateQrcodeResponse>> {
		return post<GenerateQrcodeResponse>(
			`/login/qrcode/generate?platform=${platform}${forceQr ? '&force_qr=true' : ''}`
		);
	},

	async pollQrcode(
		qrcodeKey: string,
		platform: Platform = 'bilibili'
	): Promise<ApiResponse<PollQrcodeResponse>> {
		return post<PollQrcodeResponse>('/login/qrcode/poll', { qrcode_key: qrcodeKey, platform });
	}
};
