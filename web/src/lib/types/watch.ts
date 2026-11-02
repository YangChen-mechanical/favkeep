export type WatchState = 'unwatched' | 'in_progress' | 'watched';

export interface WatchSummary {
	video_id: number;
	duration: number;
	page_count: number;
	watch_state: WatchState;
	resume_page_id: number;
	resume_position_ms: number;
}

export interface WatchProgress {
	page_id: number;
	position_ms: number;
	completed: boolean;
	revision: number;
	updated_at: string;
}

export type WatchProgressUpdate = Pick<WatchProgress, 'position_ms' | 'completed' | 'revision'>;

export interface ContinueWatchingItem {
	page_id: number;
	video_id: number;
	video_name: string;
	page_name: string;
	upper_name: string;
	pid: number;
	position_ms: number;
	duration: number;
}
