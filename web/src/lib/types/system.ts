export interface DayCountPair {
	day: string;
	cnt: number;
}

export interface DashBoardResponse {
	enabled_favorites: number;
	enabled_collections: number;
	enabled_submissions: number;
	enable_watch_later: boolean;
	videos_by_day: DayCountPair[];
}

export interface SysInfo {
	timestamp: number;
	total_memory: number;
	used_memory: number;
	process_memory: number;
	used_cpu: number;
	process_cpu: number;
	total_disk: number;
	used_disk: number;
	available_disk: number;
}

export interface TaskStatus {
	is_running: boolean;
	last_run: Date | null;
	last_finish: Date | null;
	next_run: Date | null;
}
