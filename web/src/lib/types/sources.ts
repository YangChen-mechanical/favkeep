import type { FilterOption } from './config';

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

export interface FullSyncVideoSourceRequest {
	delete_local: boolean;
}

export interface FullSyncVideoSourceResponse {
	removed_count: number;
	warnings?: string[];
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

export interface UpdateVideoSourceResponse {
	ruleDisplay: string;
}
