import type {
	ApiResponse,
	ClearAndResetVideoResponse,
	ResetFilteredVideosResponse,
	ResetFilteredVideoStatusRequest,
	ResetVideoResponse,
	ResetVideoStatusRequest,
	UpdateFilteredVideoStatusRequest,
	UpdateFilteredVideoStatusResponse,
	UpdateVideoStatusRequest,
	UpdateVideoStatusResponse,
	VideoResponse,
	VideosRequest,
	VideosResponse
} from '../types';

import { get, post } from './http';

export const mediaApi = {
	async getVideos(params?: VideosRequest): Promise<ApiResponse<VideosResponse>> {
		return get<VideosResponse>('/videos', params as Record<string, unknown>);
	},

	async getVideo(id: number): Promise<ApiResponse<VideoResponse>> {
		return get<VideoResponse>(`/videos/${id}`);
	},

	async resetVideoStatus(
		id: number,
		request: ResetVideoStatusRequest
	): Promise<ApiResponse<ResetVideoResponse>> {
		return post<ResetVideoResponse>(`/videos/${id}/reset-status`, request);
	},

	async clearAndResetVideoStatus(id: number): Promise<ApiResponse<ClearAndResetVideoResponse>> {
		return post<ClearAndResetVideoResponse>(`/videos/${id}/clear-and-reset-status`);
	},

	async resetFilteredVideoStatus(
		request: ResetFilteredVideoStatusRequest
	): Promise<ApiResponse<ResetFilteredVideosResponse>> {
		return post<ResetFilteredVideosResponse>('/videos/reset-status', request);
	},

	async updateVideoStatus(
		id: number,
		request: UpdateVideoStatusRequest
	): Promise<ApiResponse<UpdateVideoStatusResponse>> {
		return post<UpdateVideoStatusResponse>(`/videos/${id}/update-status`, request);
	},

	async updateFilteredVideoStatus(
		request: UpdateFilteredVideoStatusRequest
	): Promise<ApiResponse<UpdateFilteredVideoStatusResponse>> {
		return post<UpdateFilteredVideoStatusResponse>('/videos/update-status', request);
	}
};
