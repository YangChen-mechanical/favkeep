import assert from 'node:assert/strict';
import { test } from 'node:test';
// @ts-expect-error Node runs the TypeScript source directly and requires its extension.
import { douyinMessage, sourceVideoUrl } from './platform.ts';

test('source links use the actual platform identifier and reject missing IDs', () => {
	assert.equal(
		sourceVideoUrl({ platform: 'douyin', platform_id: '123', bvid: null }),
		'https://www.douyin.com/video/123'
	);
	assert.equal(
		sourceVideoUrl({ platform: 'bilibili', platform_id: null, bvid: 'BV123' }),
		'https://www.bilibili.com/video/BV123/'
	);
	assert.equal(sourceVideoUrl({ platform: 'douyin', platform_id: null, bvid: 'BV123' }), null);
	assert.equal(sourceVideoUrl({ platform: 'bilibili', platform_id: null, bvid: null }), null);
	assert.equal(
		sourceVideoUrl({ platform: 'douyin', platform_id: 'a/b?c' }),
		'https://www.douyin.com/video/a%2Fb%3Fc'
	);
});

test('Douyin errors distinguish identity checks and keep unknown codes private', () => {
	assert.match(douyinMessage('identity_unverified'), /未能确认账号身份/);
	assert.match(douyinMessage('account_changed'), /账号不一致/);
	assert.match(douyinMessage('login_expired'), /登录已失效/);
	for (const code of ['unexpected secret detail', '__proto__', 'toString']) {
		assert.equal(douyinMessage(code), '需要检查');
	}
});
