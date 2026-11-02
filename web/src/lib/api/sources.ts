import type {
	ApiResponse,
	CollectionsResponse,
	FavoritesResponse,
	FullSyncVideoSourceRequest,
	FullSyncVideoSourceResponse,
	InsertCollectionRequest,
	InsertFavoriteRequest,
	InsertSubmissionRequest,
	UpdateVideoSourceRequest,
	UpdateVideoSourceResponse,
	UppersResponse,
	VideoSourcesDetailsResponse,
	VideoSourcesResponse
} from '../types';

import { get, post, put, request } from './http';

export const sourcesApi = {
	async getVideoSources(): Promise<ApiResponse<VideoSourcesResponse>> {
		return get<VideoSourcesResponse>('/video-sources');
	},

	async getCreatedFavorites(): Promise<ApiResponse<FavoritesResponse>> {
		return get<FavoritesResponse>('/me/favorites');
	},

	async getFollowedCollections(
		pageNum?: number,
		pageSize?: number
	): Promise<ApiResponse<CollectionsResponse>> {
		const params = {
			page_num: pageNum,
			page_size: pageSize
		};
		return get<CollectionsResponse>('/me/collections', params as Record<string, unknown>);
	},

	async getFollowedUppers(
		pageNum?: number,
		pageSize?: number,
		name?: string
	): Promise<ApiResponse<UppersResponse>> {
		const params = {
			page_num: pageNum,
			page_size: pageSize,
			name: name
		};
		return get<UppersResponse>('/me/uppers', params as Record<string, unknown>);
	},

	async insertFavorite(request: InsertFavoriteRequest): Promise<ApiResponse<boolean>> {
		return post<boolean>('/video-sources/favorites', request);
	},

	async insertCollection(request: InsertCollectionRequest): Promise<ApiResponse<boolean>> {
		return post<boolean>('/video-sources/collections', request);
	},

	async insertSubmission(request: InsertSubmissionRequest): Promise<ApiResponse<boolean>> {
		return post<boolean>('/video-sources/submissions', request);
	},

	async getVideoSourcesDetails(): Promise<ApiResponse<VideoSourcesDetailsResponse>> {
		return get<VideoSourcesDetailsResponse>('/video-sources/details');
	},

	async updateVideoSource(
		type: string,
		id: number,
		request: UpdateVideoSourceRequest
	): Promise<ApiResponse<UpdateVideoSourceResponse>> {
		return put<UpdateVideoSourceResponse>(`/video-sources/${type}/${id}`, request);
	},

	async removeVideoSource(type: string, id: number): Promise<ApiResponse<boolean>> {
		return request<boolean>(`/video-sources/${type}/${id}`, 'DELETE');
	},

	async evaluateVideoSourceRules(type: string, id: number): Promise<ApiResponse<boolean>> {
		return post<boolean>(`/video-sources/${type}/${id}/evaluate`, null);
	},

	async fullSyncVideoSource(
		type: string,
		id: number,
		data: FullSyncVideoSourceRequest
	): Promise<ApiResponse<FullSyncVideoSourceResponse>> {
		return post<FullSyncVideoSourceResponse>(`/video-sources/${type}/${id}/full-sync`, data);
	},

	async getDefaultPath(type: string, name: string): Promise<ApiResponse<string>> {
		return get<string>(`/video-sources/${type}/default-path`, { name });
	}
};
