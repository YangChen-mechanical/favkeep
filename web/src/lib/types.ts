export interface ApiResponse<T> {
	status_code: number;
	data: T;
}

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

export interface VideoSource {
	id: number;
	name: string;
}

export interface VideoSourcesResponse {
	collection: VideoSource[];
	favorite: VideoSource[];
	submission: VideoSource[];
	watch_later: VideoSource[];
}

export type Platform = 'bilibili' | 'douyin';
export interface PlatformInfo {
	id: Platform;
	name: string;
	capabilities: string[];
}
export type DownloadState = 'waiting' | 'succeeded' | 'failed';
export type PlatformDetails =
	| { platform: 'bilibili'; download_status: [number, number, number, number, number] }
	| { platform: 'douyin'; failures: number };
export interface MediaInfo {
	download_state: DownloadState;
	media_ready: boolean;
	source_url: string | null;
	allowed_actions: string[];
	platform_details: PlatformDetails;
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

export type WatchState = 'unwatched' | 'in_progress' | 'watched';
export interface WatchSummary {
	video_id: number;
	duration: number;
	page_count: number;
	watch_state: WatchState;
	resume_page_id: number;
	resume_position_ms: number;
}

export interface PageInfo extends MediaInfo {
	id: number;
	pid: number;
	name: string;
	duration: number;
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

export interface ApiError {
	message: string;
	status: number;
}

export interface FullSyncVideoSourceRequest {
	delete_local: boolean;
}

export interface FullSyncVideoSourceResponse {
	removed_count: number;
	warnings?: string[];
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

export type Followed =
	| {
			type: 'favorite';
			title: string;
			media_count: number;
			fid: number;
			mid: number;
			invalid: boolean;
			subscribed: boolean;
	  }
	| {
			type: 'collection';
			title: string;
			sid: number;
			mid: number;
			media_count: number;
			invalid: boolean;
			subscribed: boolean;
	  }
	| {
			type: 'upper';
			mid: number;
			uname: string;
			face: string;
			sign: string;
			invalid: boolean;
			subscribed: boolean;
	  };

export interface FavoritesResponse {
	favorites: Followed[];
}

export interface CollectionsResponse {
	collections: Followed[];
	total: number;
}

export interface UppersResponse {
	uppers: Followed[];
	total: number;
}

export interface InsertFavoriteRequest {
	fid: number;
	path: string;
}

export interface InsertCollectionRequest {
	sid: number;
	mid: number;
	collection_type?: number;
	path: string;
}

export interface InsertSubmissionRequest {
	upper_id: number;
	path: string;
}

export interface Condition<T> {
	operator: string;
	value: T | T[];
}

export interface RuleTarget<T> {
	field: string;
	rule: Condition<T> | RuleTarget<T>;
}

export type AndGroup = RuleTarget<string | number | boolean | Date>[];
export type Rule = AndGroup[];

export interface VideoSourceDetail {
	id: number;
	name: string;
	path: string;
	rule: Rule | null;
	ruleDisplay: string | null;
	filterOption: FilterOption | null;
	useDynamicApi: boolean | null;
	enabled: boolean;
	latestRowAt: string | null;
}

export interface VideoSourcesDetailsResponse {
	collections: VideoSourceDetail[];
	favorites: VideoSourceDetail[];
	submissions: VideoSourceDetail[];
	watch_later: VideoSourceDetail[];
}

export interface UpdateVideoSourceRequest {
	path: string;
	enabled: boolean;
	rule?: Rule | null;
	filterOption?: FilterOption | null;
	useDynamicApi?: boolean | null;
}

export interface Credential {
	sessdata: string;
	bili_jct: string;
	buvid3: string;
	dedeuserid: string;
	ac_time_value: string;
}

export interface FilterOption {
	video_max_quality: string;
	video_min_quality: string;
	audio_max_quality: string;
	audio_min_quality: string;
	codecs: string[];
	no_dolby_video: boolean;
	no_dolby_audio: boolean;
	no_hdr: boolean;
	no_hires: boolean;
}

export interface DanmakuOption {
	duration: number;
	font: string;
	font_size: number;
	width_ratio: number;
	horizontal_gap: number;
	lane_size: number;
	float_percentage: number;
	bottom_percentage: number;
	opacity: number;
	bold: boolean;
	outline: number;
	time_offset: number;
}

export interface DanmakuUpdatePolicy {
	enabled: boolean;
	milestones: DanmakuUpdateMilestone[];
}

export type DanmakuUpdateMilestone =
	| { type: 'once'; at_days: number }
	| { type: 'periodic'; until_days: number; interval_hours: number };

export interface SkipOption {
	no_poster: boolean;
	no_video_nfo: boolean;
	no_upper: boolean;
	no_danmaku: boolean;
	no_subtitle: boolean;
}

export interface RateLimit {
	limit: number;
	duration: number;
}

export interface ConcurrentDownloadLimit {
	enable: boolean;
	concurrency: number;
	threshold: number;
}

export interface ConcurrentLimit {
	video: number;
	page: number;
	rate_limit?: RateLimit;
	download: ConcurrentDownloadLimit;
}

export interface TelegramNotifier {
	type: 'telegram';
	bot_token?: string;
	chat_id: string;
	skip_image: boolean;
}

export interface WebhookNotifier {
	type: 'webhook';
	url?: string;
	template?: string | null;
	headers?: Record<string, string> | null;
}

export interface SmtpNotifier {
	type: 'smtp';
	host: string;
	port: number;
	encryption: 'none' | 'tls' | 'starttls';
	username: string;
	password?: string;
	from: string;
	to: string;
}

export type Notifier = (TelegramNotifier | WebhookNotifier | SmtpNotifier) & {
	index?: number;
	configured?: Record<string, boolean>;
};
export interface NotifiersResponse {
	version: number;
	items: Notifier[];
}

export type Trigger = number | string;

export interface Config {
	bind_address: string;
	credential_configured: boolean;
	filter_option: FilterOption;
	danmaku_option: DanmakuOption;
	danmaku_update_policy: DanmakuUpdatePolicy;
	skip_option: SkipOption;
	video_name: string;
	page_name: string;
	ignore_common_errors: boolean;
	favorite_default_path: string;
	collection_default_path: string;
	submission_default_path: string;
	interval: Trigger;
	upper_path: string;
	nfo_time_type: string;
	concurrent_limit: ConcurrentLimit;
	time_format: string;
	cdn_sorting: boolean;
	try_upower_anyway: boolean;
	version: number;
}

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

export interface UpdateVideoSourceResponse {
	ruleDisplay: string;
}

export interface DouyinStatus {
	connected: boolean;
	account_name: string | null;
	auth_state: 'disconnected' | 'connected' | 'paused';
	interval: number;
	paused_reason: string | null;
	rejections: number;
	task: {
		is_running: boolean;
		current?: {
			operation: 'login' | 'refresh' | 'sync' | 'resume';
			stage: string;
			folder: string | null;
			video: string | null;
		} | null;
		last_sync?: {
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
		} | null;
		last_run: string | null;
		last_finish: string | null;
		next_run: string | null;
		last_error: string | null;
	};
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

// 扫码登录相关类型
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
