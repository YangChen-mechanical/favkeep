import type { MediaInfo, Platform } from './platforms';
import type { WatchProgress, WatchState, WatchSummary } from './watch';

export interface VideosRequest {
	platform?: Platform;
	saved_only?: boolean;
	watch_state?: WatchState;
	sort?: 'added' | 'title';
	collection?: number;
	favorite?: number;
	submission?: number;
	watch_later?: number;
	query?: string;
	status_filter?: 'failed' | 'succeeded' | 'waiting';
	validation_filter?: 'skipped' | 'invalid' | 'normal';
	created_from?: string;
	created_to?: string;
	page?: number;
	page_size?: number;
}

export interface VideoInfo extends MediaInfo {
	id: number;
	platform: Platform;
	platform_id: string | null;
	bvid: string | null;
	name: string;
	upper_name: string;
	valid: boolean;
	should_download: boolean;
	collection_id?: number;
	favorite_id?: number;
	submission_id?: number;
	watch_later_id?: number;
}

export interface VideosResponse {
	videos: VideoInfo[];
	total_count: number;
	watch_summaries?: WatchSummary[];
}

export interface PageInfo extends MediaInfo {
	id: number;
	pid: number;
	name: string;
	duration: number;
}

export interface VideoResponse {
	video: VideoInfo;
	pages: PageInfo[];
	watch_progress?: WatchProgress[];
	playable_page_ids?: number[];
	watch_summary?: WatchSummary | null;
}

export interface ResetVideoResponse {
	resetted: boolean;
	video: VideoInfo;
	pages: PageInfo[];
}

export interface ClearAndResetVideoResponse {
	warning?: string;
	video: VideoInfo;
}

export interface ResetFilteredVideosResponse {
	resetted: boolean;
	resetted_videos_count: number;
	resetted_pages_count: number;
}

export interface UpdateVideoStatusResponse {
	success: boolean;
	video: VideoInfo;
	pages: PageInfo[];
}

export interface UpdateFilteredVideoStatusResponse {
	success: boolean;
	updated_videos_count: number;
	updated_pages_count: number;
}

export interface StatusUpdate {
	status_index: number;
	status_value: number;
}

export interface PageStatusUpdate {
	page_id: number;
	updates: StatusUpdate[];
}

export interface UpdateVideoStatusRequest {
	video_updates?: StatusUpdate[];
	page_updates?: PageStatusUpdate[];
}

export interface UpdateFilteredVideoStatusRequest {
	platform?: Platform;
	collection?: number;
	favorite?: number;
	submission?: number;
	watch_later?: number;
	query?: string;
	status_filter?: 'failed' | 'succeeded' | 'waiting';
	validation_filter?: 'skipped' | 'invalid' | 'normal';
	created_from?: string;
	created_to?: string;
	video_updates?: StatusUpdate[];
	page_updates?: StatusUpdate[];
}

export interface ResetVideoStatusRequest {
	force: boolean;
}

export interface ResetFilteredVideoStatusRequest {
	platform?: Platform;
	collection?: number;
	favorite?: number;
	submission?: number;
	watch_later?: number;
	query?: string;
	status_filter?: 'failed' | 'succeeded' | 'waiting';
	validation_filter?: 'skipped' | 'invalid' | 'normal';
	created_from?: string;
	created_to?: string;
	force: boolean;
}
