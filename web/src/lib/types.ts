export interface ApiResponse<T> {
	status_code: number;
	data: T;
}

export interface ApiError {
	message: string;
	status: number;
}

export type * from './types/config';
export type * from './types/media';
export type * from './types/platforms';
export type * from './types/sources';
export type * from './types/system';
export type * from './types/watch';
