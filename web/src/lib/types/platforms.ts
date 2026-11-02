export type Platform = 'bilibili' | 'douyin' | 'xiaohongshu';

export interface PlatformInfo {
	id: Platform;
	name: string;
	capabilities: string[];
}

export type DownloadState = 'waiting' | 'succeeded' | 'failed';

export type PlatformDetails =
	| { platform: 'bilibili'; download_status: [number, number, number, number, number] }
	| { platform: 'douyin'; failures: number }
	| { platform: 'xiaohongshu'; failures: number };

export interface MediaInfo {
	download_state: DownloadState;
	media_ready: boolean;
	source_url: string | null;
	allowed_actions: string[];
	platform_details: PlatformDetails;
}

export interface SyncSummary {
	started_at: string;
	finished_at: string | null;
	scanned: number;
	historical: number;
	existing: number;
	unsupported: number;
	inserted: number;
	downloaded: number;
	reused: number;
	recovered: number;
	failed: number;
	pending: number | null;
	error: string | null;
}

export interface SyncTask<T extends SyncSummary = SyncSummary> {
	is_running: boolean;
	current?: {
		operation: 'login' | 'refresh' | 'sync' | 'resume';
		stage: string;
		folder: string | null;
		video: string | null;
	} | null;
	last_sync?: T | null;
	last_run: string | null;
	last_finish: string | null;
	next_run: string | null;
	last_error: string | null;
}

export interface PlatformStatus {
	connected: boolean;
	account_name: string | null;
	auth_state: 'disconnected' | 'connected' | 'paused';
	interval: number;
	paused_reason: string | null;
	rejections: number;
	task: SyncTask;
}

export type DouyinStatus = PlatformStatus;

export interface WindowSyncSummary extends SyncSummary {
	window_size: number;
	baseline: boolean;
	rebased: boolean;
}

export interface XiaohongshuStatus extends PlatformStatus {
	window_size: number;
	next_allowed_at: string | null;
	task: SyncTask<WindowSyncSummary>;
}

export interface XiaohongshuSource {
	name: string;
	enabled: boolean;
	revision: number;
	baseline_done: boolean;
	baseline_at: number | null;
	last_scan: number | null;
	last_result: string | null;
	item_count: number;
	window_size: number;
}

export interface DouyinFolder {
	id: string;
	name: string;
	enabled: boolean;
	available: boolean;
	baseline_at: string | null;
	last_sync_at: string | null;
	last_error: string | null;
	video_count: number;
}

export interface DouyinFoldersResponse {
	folders: DouyinFolder[];
}

export interface QrcodeGenerateResponse {
	url?: string;
	image?: string;
	qrcode_key: string;
	message?: string;
}

export type QrcodePollResponse =
	| {
			status: 'success';
			version?: number;
	  }
	| {
			status: 'pending';
			message: string;
			scanned?: boolean;
			image?: string;
			url?: string;
	  }
	| {
			status: 'expired';
			message: string;
	  }
	| {
			status: 'failed' | 'verification_required' | 'account_changed';
			message: string;
	  };
