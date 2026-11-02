import { writable } from 'svelte/store';
import type { ApiResponse, DouyinStatus, XiaohongshuStatus } from '../types';

export type SyncStatus = DouyinStatus | XiaohongshuStatus;

export interface SyncActions {
	name: string;
	getStatus: () => Promise<ApiResponse<SyncStatus>>;
	updateInterval: (seconds: number) => Promise<ApiResponse<SyncStatus>>;
	trigger: () => Promise<unknown>;
	resume: () => Promise<unknown>;
}

export function createSyncTask(actions: SyncActions) {
	const { subscribe, update, set } = writable({
		status: null as SyncStatus | null,
		error: '',
		busy: false,
		interval: '',
		intervalLoaded: false,
		intervalError: '',
		intervalSaving: false
	});
	let active = true;
	let revision = 0;
	const message = (error: unknown) =>
		error instanceof Error
			? error.message
			: ((error as { message?: string })?.message ?? '请求失败');

	async function loadStatus() {
		const current = ++revision;
		try {
			const { data } = await actions.getStatus();
			if (!data?.task || typeof data.task.is_running !== 'boolean') {
				throw new Error(`${actions.name}任务状态无效，请刷新重试`);
			}
			if (active && current === revision)
				update((state) => ({ ...state, status: data, error: '' }));
		} catch (error) {
			if (active && current === revision) update((state) => ({ ...state, error: message(error) }));
		}
	}

	async function loadInterval() {
		try {
			const { data } = await actions.getStatus();
			if (active)
				update((state) => ({
					...state,
					interval: String(data.interval),
					intervalLoaded: true,
					intervalError: ''
				}));
		} catch (error) {
			if (active) update((state) => ({ ...state, intervalError: message(error) }));
		}
	}

	async function saveInterval(value: string) {
		if (!value.trim()) throw new Error('请输入同步间隔');
		const seconds = Number(value.trim());
		if (!Number.isSafeInteger(seconds) || seconds < 61 || seconds > 31536000) {
			throw new Error(`${actions.name}同步间隔须为 61 至 31536000 的整数秒`);
		}
		update((state) => ({ ...state, intervalSaving: true }));
		try {
			const { data } = await actions.updateInterval(seconds);
			++revision;
			if (active) update((state) => ({ ...state, status: data }));
		} finally {
			if (active) update((state) => ({ ...state, intervalSaving: false }));
		}
	}

	async function run(resume = false) {
		update((state) => ({ ...state, busy: true }));
		try {
			await (resume ? actions.resume() : actions.trigger());
			await loadStatus();
		} finally {
			if (active) update((state) => ({ ...state, busy: false }));
		}
	}

	function start() {
		active = true;
		void loadInterval();
		void loadStatus();
		const timer = setInterval(loadStatus, 5000);
		return () => {
			active = false;
			++revision;
			clearInterval(timer);
		};
	}

	return { subscribe, set, start, loadStatus, loadInterval, saveInterval, run };
}
