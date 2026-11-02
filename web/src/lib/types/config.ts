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
	xiaohongshu: { interval: number; browser: string; path: string };
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
