export interface ApiResponse<T> {
	status_code: number;
	data: T;
}

export interface ApiError {
	message: string;
	status: number;
}
