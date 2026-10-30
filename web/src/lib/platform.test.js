import assert from 'node:assert/strict';
import { test } from 'node:test';
// @ts-expect-error Node runs the TypeScript source directly and requires its extension.
import { douyinMessage } from './platform.ts';

test('Douyin errors distinguish identity checks and keep unknown codes private', () => {
	assert.match(douyinMessage('identity_unverified'), /未能确认账号身份/);
	assert.match(douyinMessage('account_changed'), /账号不一致/);
	assert.match(douyinMessage('login_expired'), /登录已失效/);
	for (const code of ['unexpected secret detail', '__proto__', 'toString']) {
		assert.equal(douyinMessage(code), '需要检查');
	}
});
