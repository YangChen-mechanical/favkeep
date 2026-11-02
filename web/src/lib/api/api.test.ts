import assert from 'node:assert/strict';
import { after, test } from 'node:test';
import { get } from 'svelte/store';
import { authSession } from '../stores/auth';

const storage = Object.getOwnPropertyDescriptor(globalThis, 'localStorage');
Object.defineProperty(globalThis, 'localStorage', {
	configurable: true,
	value: { removeItem() {} }
});
const { default: api } = await import('../api');
const originalFetch = globalThis.fetch;
const requests: { url: string; init?: RequestInit }[] = [];
let response = { status: 200, data: {} as unknown };
globalThis.fetch = async (input, init) => {
	requests.push({ url: String(input), init });
	return new Response(JSON.stringify({ status_code: response.status, data: response.data }), {
		status: response.status,
		headers: { 'Content-Type': 'application/json' }
	});
};
after(() => {
	globalThis.fetch = originalFetch;
	if (storage) Object.defineProperty(globalThis, 'localStorage', storage);
	else Reflect.deleteProperty(globalThis, 'localStorage');
	authSession.set({ setup_required: false, authenticated: false });
});

test('domain APIs keep query values, encoded IDs, credentials and CSRF', async () => {
	authSession.set({ setup_required: false, authenticated: true, csrf_token: 'test-csrf' });
	await api.getVideos({ platform: 'douyin', page: 0, saved_only: false });
	assert.equal(requests.at(-1)?.url, '/api/videos?platform=douyin&page=0&saved_only=false');
	assert.equal(requests.at(-1)?.init?.credentials, 'same-origin');
	assert.equal(new Headers(requests.at(-1)?.init?.headers).has('X-CSRF-Token'), false);
	await api.updateDouyinFolder('a/b?c', true);
	assert.equal(requests.at(-1)?.url, '/api/video-sources/douyin/a%2Fb%3Fc');
	assert.equal(requests.at(-1)?.init?.method, 'PUT');
	assert.equal(new Headers(requests.at(-1)?.init?.headers).get('X-CSRF-Token'), 'test-csrf');
	assert.equal(requests.at(-1)?.init?.body, '{"enabled":true}');
	await api.saveWatchProgress(12, { position_ms: 1000, completed: false, revision: 2 }, true);
	assert.equal(requests.at(-1)?.url, '/api/watch-progress/12');
	assert.equal(requests.at(-1)?.init?.keepalive, true);
});

test('auth and platform methods retain endpoints and unauthorized responses revoke local state', async () => {
	response = {
		status: 200,
		data: { setup_required: false, authenticated: true, csrf_token: 'renewed' }
	};
	await api.login('admin', 'test password');
	assert.equal(requests.at(-2)?.url, '/api/auth/login');
	assert.equal(requests.at(-1)?.url, '/api/auth/session');
	assert.equal(get(authSession)?.csrf_token, 'renewed');
	await api.generateQrcode('xiaohongshu', true);
	assert.equal(
		requests.at(-1)?.url,
		'/api/login/qrcode/generate?platform=xiaohongshu&force_qr=true'
	);
	await api.cancelQrcode('test-key', 'douyin');
	assert.equal(
		requests.at(-1)?.init?.body,
		'{"qrcode_key":"test-key","platform":"douyin","cancel":true}'
	);
	response = { status: 401, data: null };
	await assert.rejects(api.getVideo(1), (error: { status: number }) => error.status === 401);
	assert.equal(get(authSession)?.authenticated, false);
});
