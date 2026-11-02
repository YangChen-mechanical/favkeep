import { get as getStore } from 'svelte/store';
import { authSession } from '../stores/auth';
import type { ApiError, ApiResponse } from '../types';
import { wsManager } from '../ws';

const API_BASE_URL = '/api';

export async function request<T>(
	url: string,
	method: string = 'GET',
	body?: unknown,
	params?: Record<string, unknown>,
	keepalive = false
): Promise<ApiResponse<T>> {
	// 构建完整的 URL
	let fullUrl = `${API_BASE_URL}${url}`;
	if (params) {
		const searchParams = new URLSearchParams();
		Object.entries(params).forEach(([key, value]) => {
			if (value !== undefined && value !== null) {
				searchParams.append(key, String(value));
			}
		});
		const queryString = searchParams.toString();
		if (queryString) {
			fullUrl += `?${queryString}`;
		}
	}

	const config: RequestInit = {
		method,
		keepalive,
		credentials: 'same-origin',
		headers: {
			'Content-Type': 'application/json',
			...(getStore(authSession)?.csrf_token && method !== 'GET' && method !== 'HEAD'
				? { 'X-CSRF-Token': getStore(authSession)!.csrf_token! }
				: {})
		}
	};

	if (body && method !== 'GET') {
		config.body = JSON.stringify(body);
	}

	try {
		const response = await fetch(fullUrl, config);

		if (!response.ok) {
			if (response.status === 401 && url !== '/auth/login') {
				authSession.set({ setup_required: false, authenticated: false });
				wsManager.disconnect();
			}
			const errorText = await response.text();
			let errorMessage: string;
			try {
				const errorJson = JSON.parse(errorText);
				errorMessage = errorJson.message || errorJson.error || '请求失败';
			} catch {
				errorMessage = errorText || `HTTP ${response.status}: ${response.statusText}`;
			}
			throw {
				message: errorMessage,
				status: response.status
			} as ApiError;
		}

		return await response.json();
	} catch (error) {
		if (error && typeof error === 'object' && 'status' in error) {
			throw error;
		}
		throw {
			message: error instanceof Error ? error.message : '网络请求失败',
			status: 0
		} as ApiError;
	}
}

export async function get<T>(
	url: string,
	params?: Record<string, unknown>
): Promise<ApiResponse<T>> {
	return request<T>(url, 'GET', undefined, params);
}

export async function post<T>(url: string, data?: unknown): Promise<ApiResponse<T>> {
	return request<T>(url, 'POST', data);
}

export async function put<T>(url: string, data?: unknown): Promise<ApiResponse<T>> {
	return request<T>(url, 'PUT', data);
}
