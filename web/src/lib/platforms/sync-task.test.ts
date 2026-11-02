import assert from 'node:assert/strict';
import { test } from 'node:test';
import { get } from 'svelte/store';
import type { ApiResponse } from '../types';
import { createSyncTask, type SyncActions, type SyncStatus } from './sync-task';

const status = (interval = 1800): SyncStatus => ({
	connected: true,
	auth_state: 'connected',
	account_name: null,
	paused_reason: null,
	rejections: 0,
	interval,
	task: { is_running: false, last_run: null, last_finish: null, next_run: null, last_error: null }
});

function fixture(overrides: Partial<SyncActions> = {}) {
	const calls: (string | number)[] = [];
	const actions: SyncActions = {
		name: '抖音',
		getStatus: async () => ({ status_code: 200, data: status() }),
		updateInterval: async (seconds) => {
			calls.push(seconds);
			return { status_code: 200, data: status(seconds) };
		},
		trigger: async () => {
			calls.push('trigger');
		},
		resume: async () => {
			calls.push('resume');
		},
		...overrides
	};
	return { task: createSyncTask(actions), calls };
}

test('interval validation prevents invalid requests and keeps both endpoints', async () => {
	const { task, calls } = fixture();
	for (const value of ['', '60', '31536001', '1.5', 'NaN', 'Infinity']) {
		await assert.rejects(task.saveInterval(value));
	}
	assert.deepEqual(calls, []);
	await task.saveInterval(' 61 ');
	await task.saveInterval('31536000');
	assert.deepEqual(calls, [61, 31536000]);
	assert.equal(get(task).status?.interval, 31536000);
	assert.equal(get(task).intervalSaving, false);
});

test('reload errors retain the last status and failed actions release busy state', async () => {
	let fail = false;
	const { task, calls } = fixture({
		getStatus: async () => {
			if (fail) throw { message: '离线' };
			return { status_code: 200, data: status() };
		},
		resume: async () => {
			throw new Error('恢复失败');
		}
	});
	await task.loadStatus();
	fail = true;
	await task.loadStatus();
	assert.equal(get(task).status?.connected, true);
	assert.equal(get(task).error, '离线');
	await task.run();
	assert.deepEqual(calls, ['trigger']);
	await assert.rejects(task.run(true), /恢复失败/);
	assert.equal(get(task).busy, false);
});

function deferred() {
	let resolve!: (value: ApiResponse<SyncStatus>) => void;
	const promise = new Promise<ApiResponse<SyncStatus>>((done) => {
		resolve = done;
	});
	return { promise, resolve };
}

test('saving invalidates stale polling and stopping ignores outstanding requests', async () => {
	let response = deferred();
	const { task } = fixture({ getStatus: () => response.promise });
	const pending = task.loadStatus();
	await task.saveInterval('120');
	response.resolve({ status_code: 200, data: status(1800) });
	await pending;
	assert.equal(get(task).status?.interval, 120);
	response = deferred();
	const stop = task.start();
	const pendingAfterStop = task.loadStatus();
	stop();
	response.resolve({ status_code: 200, data: status(900) });
	await pendingAfterStop;
	assert.equal(get(task).status?.interval, 120);
});
