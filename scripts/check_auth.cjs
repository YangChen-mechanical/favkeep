const assert = require('node:assert/strict');
const path = require('node:path');
const fs = require('node:fs/promises');
const { pathToFileURL } = require('node:url');
const { chromium, expect: playwrightExpect } = require('playwright/test');
const expect = playwrightExpect.configure({ timeout: 15000 });
const origin = process.env.FAVKEEP_TEST_ORIGIN || 'http://127.0.0.1:5183';
// Run against a production preview. This checks frontend behavior with a simulated API.
(async () => {
    const { returnPath } = await import(pathToFileURL(path.resolve(__dirname, '../web/src/lib/stores/auth.ts')).href);
    for (const value of ['//evil.example', '/\\evil.example', '/\n/evil.example', '/login', '/setup?x=1', 'https://evil.example']) assert.equal(returnPath(value), '/watch');
    assert.equal(returnPath('/watch/1?part=2'), '/watch/1?part=2');
    const browser = await chromium.launch({ channel: 'msedge', headless: true });
    try {
        const context = await browser.newContext({ viewport: { width: 390, height: 844 } });
        const page = await context.newPage();
        const failures = [];
        page.on('pageerror', error => failures.push(error.message));
        let initialized = false;
        let loggedIn = false;
        let expired = false;
        let businessBeforeLogin = 0;
        let logoutRequests = 0;
        await context.addInitScript(() => localStorage.setItem('authToken', 'legacy-token'));
        await context.routeWebSocket('**/api/ws', socket => socket.onMessage(() => {}));
        await context.route('**/api/**', async route => {
            const request = route.request();
            const url = new URL(request.url());
            const json = (data, status=200) => route.fulfill({ status, json: { status_code: status, data } });
            assert.equal(request.headers().authorization, undefined);
            if (url.pathname === '/api/auth/session') return json({ setup_required: !initialized, authenticated: loggedIn && !expired, ...(loggedIn && !expired ? { username: 'admin', csrf_token: 'test-csrf' } : {}) });
            if (url.pathname === '/api/auth/setup') {
                assert.equal(request.headers().origin, origin);
                const body = request.postDataJSON();
                assert.equal(body.setup_code, 'setup-code');
                initialized = true; loggedIn = true;
                return json(null);
            }
            if (url.pathname === '/api/auth/login') {
                const body=request.postDataJSON();
                if (body.password !== 'correct horse battery staple') return route.fulfill({ status: 401, json: { message: '用户名或密码错误' } });
                loggedIn = true; expired = false;
                return json(null);
            }
            if (url.pathname === '/api/auth/logout') {
                assert.equal(request.headers()['x-csrf-token'], 'test-csrf');
                assert.equal(request.headers().origin, origin);
                loggedIn = false; logoutRequests++;
                return json(null);
            }
            if (!loggedIn) businessBeforeLogin++;
            if (!loggedIn || expired) return route.fulfill({ status: 401, json: { message: '请先登录' } });
            if (url.pathname === '/api/videos') return json({ videos: [], total_count: 0 });
            if (url.pathname === '/api/continue-watching') return json([]);
            if (url.pathname.startsWith('/api/media/')) return route.fulfill({ status: 404 });
            return json({});
        });
        const output = path.resolve(__dirname, '../local-data/auth-smoke');
        await fs.mkdir(output, { recursive: true });
        await page.goto(origin + '/watch');
        await expect(page).toHaveURL(origin + '/setup');
        assert.equal(businessBeforeLogin, 0);
        await expect(page.getByRole('heading', { name: '初始化管理员' })).toBeVisible();
        await page.screenshot({ path: path.join(output, 'setup-mobile.png'), fullPage: true });
        await page.getByLabel('初始化码', { exact: true }).fill('setup-code');
        await page.getByLabel('用户名', { exact: true }).fill('admin');
        await page.getByLabel('密码', { exact: true }).fill('correct horse battery staple');
        await page.getByLabel('再次输入密码', { exact: true }).fill('wrong confirmation');
        await page.getByRole('button', { name: '创建管理员', exact: true }).click();
        await expect(page.getByRole('alert')).toHaveText('两次密码不一致');
        await page.getByLabel('再次输入密码', { exact: true }).fill('correct horse battery staple');
        await page.getByRole('button', { name: '创建管理员', exact: true }).click();
        await expect(page).toHaveURL(origin + '/watch');
        assert.equal(await page.evaluate(() => localStorage.getItem('authToken')), null);
        await page.reload();
        await expect(page.getByRole('button', { name: '退出登录', exact: true })).toBeVisible();
        await page.getByRole('button', { name: '退出登录', exact: true }).click();
        await expect(page).toHaveURL(/\/login\?returnTo=/);
        assert.equal(logoutRequests, 1);
        await page.getByLabel('用户名', { exact: true }).fill('admin');
        await page.getByLabel('密码', { exact: true }).fill('wrong password');
        await page.getByRole('button', { name: '登录', exact: true }).click();
        await expect(page.getByRole('alert')).toHaveText('用户名或密码错误');
        await page.screenshot({ path: path.join(output, 'login-mobile.png'), fullPage: true });
        await page.getByLabel('密码', { exact: true }).fill('correct horse battery staple');
        await page.getByRole('button', { name: '登录', exact: true }).click();
        await expect(page).toHaveURL(origin + '/watch');
        expired = true;
        await page.getByRole('link', { name: '进入管理', exact: true }).click();
        await expect(page).toHaveURL(/\/login\?returnTo=/);
        await page.setViewportSize({ width: 1440, height: 1000 });
        await page.screenshot({ path: path.join(output, 'login-desktop.png'), fullPage: true });
        assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
        assert.deepEqual(failures, []);
        assert.equal(businessBeforeLogin, 0);
        console.log('PASS: setup protection UI, login errors, session restore, CSRF header, logout, 401 redirect, old token removal, safe return URLs and mobile layout.');
    } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
