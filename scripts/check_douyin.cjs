const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const { chromium, expect: playwrightExpect } = require('playwright/test');
const expect = playwrightExpect.configure({ timeout: 15000 });
const origin = process.env.FAVKEEP_TEST_ORIGIN || 'http://127.0.0.1:5187';

// Production-preview UI checks with a synthetic API. No platform requests or credentials.
(async () => {
  assert.ok(process.argv[2], 'Usage: node scripts/check_douyin.cjs <local MP4 sample>');
  const media = await fs.readFile(process.argv[2]);
  const output = path.resolve(__dirname, '../local-data/douyin-ui');
  await fs.mkdir(output, { recursive: true });
  const browser = await chromium.launch({ channel: 'msedge', headless: true });
  try {
    const context = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
    const page = await context.newPage();
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    const config = {
      version: 1, bind_address: '127.0.0.1:12345', credential_configured: true,
      interval: 1200, video_name: 'title', page_name: 'title', upper_path: '/data/uppers',
      favorite_default_path: '/data/favorites', collection_default_path: '/data/collections',
      submission_default_path: '/data/submissions', ignore_common_errors: true,
      nfo_time_type: 'pubtime', time_format: '%Y-%m-%d', cdn_sorting: false, try_upower_anyway: false,
      filter_option: { video_max_quality: '8k', video_min_quality: '360p', audio_max_quality: 'hires', audio_min_quality: '64k', codecs: ['avc'], no_dolby_video: false, no_dolby_audio: false, no_hdr: false, no_hires: false },
      danmaku_option: { duration: 15, font: 'sans-serif', font_size: 25, width_ratio: 1, horizontal_gap: 1, lane_size: 1, float_percentage: 0.5, bottom_percentage: 0.5, opacity: 1, bold: false, outline: 1, time_offset: 0 },
      danmaku_update_policy: { enabled: false, milestones: [] },
      skip_option: { no_poster: false, no_video_nfo: false, no_upper: false, no_danmaku: false, no_subtitle: false },
      concurrent_limit: { video: 1, page: 1, download: { enable: false, concurrency: 1, threshold: 1 } }
    };
    const status = {
      connected: true, account_name: null, auth_state: 'connected', interval: 1800,
      paused_reason: null, rejections: 0,
      task: { is_running: false, last_run: '2026-10-26T12:00:00Z', last_finish: '2026-10-26T12:01:00Z', next_run: '2026-10-26T12:31:00Z', last_error: null }
    };
    const folders = [
      { id: '111', name: '抖音精选', enabled: true, baseline_at: '2026-10-25T12:00:00Z', last_sync_at: '2026-10-26T12:01:00Z', last_error: null, video_count: 3 },
      { id: '222', name: '稍后慢慢看', enabled: false, baseline_at: null, last_sync_at: null, last_error: null, video_count: 0 },
      { id: '333', name: '失效收藏夹', enabled: false, available: false, baseline_at: null, last_sync_at: null, last_error: null, video_count: 0 }
    ];
    const videos = [
      { id: 1, platform: 'bilibili', platform_id: 'BVfixture', bvid: 'BVfixture', name: 'B站测试视频', upper_name: 'B站作者' },
      { id: 2, platform: 'douyin', platform_id: '1234567890123456789', bvid: null, name: '抖音测试视频', upper_name: '抖音作者' }
    ].map(video => ({ ...video, valid: true, should_download: true, download_status: [7, 7, 7, 7, 7] }));
    let refreshes = 0;
    let qrGenerations = 0;
    let qrPolls = 0;
    let qrMode = 'success';
    let resumes = 0;
    let runs = 0;
    let progress = null;
    const changes = [];
    const queries = [];
    const qrImage = 'data:image/svg+xml;base64,' + Buffer.from('<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><rect width="100" height="100" fill="white"/></svg>').toString('base64');
    await context.routeWebSocket('**/api/ws', socket => socket.onMessage(() => {}));
    await context.route('**/api/**', async route => {
      const request = route.request();
      const url = new URL(request.url());
      const json = data => route.fulfill({ json: { status_code: 200, data } });
      if (request.method() !== 'GET') {
        assert.equal(request.headers()['x-csrf-token'], 'douyin-smoke-csrf');
        assert.equal(request.headers().origin, origin);
      }
      if (url.pathname === '/api/auth/session') return json({ setup_required: false, authenticated: true, username: 'smoke', csrf_token: 'douyin-smoke-csrf' });
      if (url.pathname === '/api/config') return json(config);
      if (url.pathname === '/api/config/notifiers') return json({ version: config.version, items: [] });
      if (url.pathname === '/api/douyin/status') return json(status);
      if (url.pathname === '/api/config/douyin') {
        status.interval = request.postDataJSON().interval;
        assert.equal(config.interval, 1200, 'Douyin interval must not alter Bili interval');
        config.version++;
        return json(status);
      }
      if (url.pathname === '/api/login/qrcode/generate') {
        assert.equal(url.searchParams.get('platform'), 'douyin');
        qrGenerations++;
        return json({ qrcode_key: 'synthetic-key', url: '', status: 'pending', message: '正在继续本次登录请求' });
      }
      if (url.pathname === '/api/login/qrcode/poll') {
        assert.deepEqual(request.postDataJSON(), { qrcode_key: 'synthetic-key', platform: 'douyin' });
        qrPolls++;
        return json(qrMode === 'success' ? { status: 'success' } : qrMode === 'failed' ? { status: 'verification_required', message: '请在服务器本机完成验证后重试' } : { status: 'pending', message: '请扫码确认', image: qrImage });
      }
      if (url.pathname === '/api/video-sources/details') return json({ favorites: [], collections: [], submissions: [], watch_later: [] });
      if (url.pathname === '/api/video-sources/douyin/refresh') { refreshes++; return json({ folders }); }
      if (url.pathname === '/api/video-sources/douyin') return json({ folders });
      if (/^\/api\/video-sources\/douyin\/\d+$/.test(url.pathname)) {
        const folder = folders.find(item => item.id === url.pathname.split('/').pop());
        assert.ok(folder);
        const { enabled } = request.postDataJSON();
        assert.equal(typeof enabled, 'boolean');
        folder.enabled = enabled;
        changes.push({ id: folder.id, enabled });
        return json(folder);
      }
      if (url.pathname === '/api/douyin/resume') {
        assert.equal(status.auth_state, 'paused');
        resumes++;
        status.auth_state = 'connected'; status.paused_reason = null; status.rejections = 0; status.task.last_error = null;
        return json(true);
      }
      if (url.pathname === '/api/task/douyin') { runs++; status.task.is_running = true; return json(true); }
      if (url.pathname === '/api/dashboard') return json({ enabled_favorites: 0, enabled_collections: 0, enabled_submissions: 0, enable_watch_later: false, videos_by_day: [] });
      if (url.pathname === '/api/videos') {
        queries.push(url.searchParams);
        const selected = videos.filter(video => !url.searchParams.get('platform') || video.platform === url.searchParams.get('platform'));
        return json({ videos: selected, total_count: selected.length, watch_summaries: selected.map(video => ({ video_id: video.id, duration: 120, page_count: 1, watch_state: 'unwatched', resume_page_id: 200 + video.id, resume_position_ms: 0 })) });
      }
      if (/^\/api\/videos\/\d+$/.test(url.pathname)) {
        const video = videos.find(item => item.id === Number(url.pathname.split('/').pop()));
        return json({ video, pages: [{ id: 200 + video.id, pid: 1, name: video.name, duration: 120, download_status: [7, 7, 7, 7, 7] }], playable_page_ids: [200 + video.id], watch_progress: [], watch_summary: null });
      }
      if (url.pathname === '/api/continue-watching') return json([]);
      if (url.pathname.startsWith('/api/watch-progress/')) {
        if (request.method() === 'PUT') progress = { page_id: 202, ...request.postDataJSON(), revision: (progress?.revision || 0) + 1 };
        return json(progress);
      }
      if (/\/api\/media\/videos\/\d+\/cover$/.test(url.pathname)) return route.fulfill({ status: 404 });
      if (url.pathname.startsWith('/api/media/pages/')) {
        const range = request.headers().range;
        if (!range) return route.fulfill({ contentType: 'video/mp4', body: media });
        const [, first, last] = /bytes=(\d+)-(\d*)/.exec(range);
        const start = Number(first), end = Math.min(last ? Number(last) : media.length - 1, media.length - 1);
        return route.fulfill({ status: 206, contentType: 'video/mp4', headers: { 'Content-Range': `bytes ${start}-${end}/${media.length}`, 'Accept-Ranges': 'bytes' }, body: media.subarray(start, end + 1) });
      }
      throw new Error(`Unexpected API: ${request.method()} ${url.pathname}`);
    });

    await page.goto(origin + '/settings');
    await page.getByRole('tab', { name: '平台连接', exact: true }).click();
    const connection = page.getByRole('region', { name: '抖音连接', exact: true });
    await expect(connection).toContainText('已连接');
    await connection.getByLabel('抖音同步间隔（秒）').fill('1801');
    await connection.getByRole('button', { name: '保存抖音间隔' }).click();
    await expect.poll(() => status.interval).toBe(1801);
    await connection.getByRole('button', { name: '重新登录', exact: true }).click();
    await expect.poll(() => qrGenerations).toBe(1);
    await expect.poll(() => qrPolls).toBeGreaterThan(0);
    await expect(page.getByRole('dialog')).toHaveCount(0);
    await expect.poll(() => refreshes).toBe(1);
    qrMode = 'pending';
    await connection.getByRole('button', { name: '重新登录', exact: true }).click();
    await expect(page.getByRole('img', { name: '登录二维码', exact: true })).toHaveAttribute('src', qrImage);
    await expect(page.getByRole('button', { name: '二维码有效中', exact: true })).toBeDisabled();
    assert.equal(qrGenerations, 2, 'Pending Douyin login must keep the current server login task');
    await expect(page.getByRole('img', { name: '登录二维码', exact: true })).toBeVisible();
    qrMode = 'failed';
    await expect(page.getByRole('dialog')).toContainText('请在服务器本机完成验证后重试');
    await expect(page.getByRole('button', { name: '重新获取二维码', exact: true })).toBeVisible();
    await page.keyboard.press('Escape');
    assert.equal(refreshes, 1, 'Pending or failed login must not refresh folders');

    await page.goto(origin + '/video-sources');
    await page.getByRole('tab', { name: '抖音收藏夹', exact: true }).click();
    const selectedFolder = page.getByRole('checkbox', { name: '同步收藏夹 抖音精选', exact: true });
    await expect(selectedFolder).toBeChecked();
    await expect(page.getByRole('row', { name: /抖音精选/ })).toContainText('同步成功');
    await expect(page.getByRole('row', { name: /稍后慢慢看/ })).toContainText('尚未建立');
    await expect(page.getByRole('row', { name: /失效收藏夹/ })).toContainText('已停止同步');
    await expect(page.getByRole('checkbox', { name: '同步收藏夹 失效收藏夹', exact: true })).toBeDisabled();
    await selectedFolder.click();
    await expect(selectedFolder).not.toBeChecked();
    await selectedFolder.click();
    await expect(selectedFolder).toBeChecked();
    assert.deepEqual(changes, [{ id: '111', enabled: false }, { id: '111', enabled: true }]);
    assert.equal(refreshes, 1, 'Viewing local folders must not contact the platform');
    await page.getByRole('button', { name: '刷新抖音收藏夹', exact: true }).click();
    await expect.poll(() => refreshes).toBe(2);

    status.auth_state = 'paused'; status.paused_reason = 'rejected'; status.rejections = 3; status.task.last_error = 'rejected'; status.task.next_run = null;
    await page.goto(origin + '/');
    const task = page.getByRole('region', { name: '抖音任务', exact: true });
    await expect(task).toContainText('连续拒绝 3 次');
    await expect(task.getByRole('button', { name: '立即同步抖音', exact: true })).toHaveCount(0);
    await task.getByRole('button', { name: '验证连接并恢复', exact: true }).click();
    await expect.poll(() => resumes).toBe(1);
    await task.getByRole('button', { name: '立即同步抖音', exact: true }).click();
    await expect.poll(() => runs).toBe(1);
    await expect(task.getByRole('button', { name: '任务运行中', exact: true })).toBeDisabled();
    status.task.is_running = false;

    await page.goto(origin + '/watch');
    const library = page.getByRole('region', { name: '已保存视频', exact: true });
    await expect(library.locator('.watch-card')).toHaveCount(2);
    await page.getByLabel('平台', { exact: true }).selectOption('douyin');
    await expect(library.locator('.watch-card')).toHaveCount(1);
    await expect.poll(() => queries.at(-1).get('platform')).toBe('douyin');
    await page.reload();
    await expect(page.getByLabel('平台', { exact: true })).toHaveValue('douyin');
    await library.locator('.watch-card').click();
    await expect(page.getByRole('link', { name: '在抖音打开', exact: true })).toHaveAttribute('href', 'https://www.douyin.com/video/1234567890123456789');
    await expect.poll(() => page.locator('video').evaluate(video => video.readyState)).toBeGreaterThanOrEqual(1);
    await page.getByRole('link', { name: '管理此视频', exact: true }).click();
    for (const name of ['重置', '清空重置', '编辑状态']) await expect(page.getByRole('button', { name, exact: true })).toHaveCount(0);
    await expect(page.getByRole('button', { name: '在抖音打开', exact: true })).toBeVisible();
    await expect(page.getByText('抖音仅保存视频内容。', { exact: false })).toBeVisible();
    for (const width of [390, 350]) {
      await page.setViewportSize({ width, height: 844 });
      for (const route of ['/settings', '/video-sources', '/', '/watch?platform=douyin', '/watch/2', '/video/2']) {
        await page.goto(origin + route);
        if (route === '/settings') await page.getByRole('tab', { name: '平台连接', exact: true }).click();
        if (route === '/video-sources') await page.getByRole('tab', { name: '抖音收藏夹', exact: true }).click();
        await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), { message: `Horizontal overflow at ${route}, width ${width}` }).toBe(true);
        if (width === 390) await page.screenshot({ path: path.join(output, `${route.split('?')[0].replaceAll('/', '-') || 'dashboard'}-mobile.png`), fullPage: true });
      }
    }
    assert.deepEqual(errors, []);
    console.log('PASS: Douyin session restore without QR, pending image/reuse/error, interval isolation, folder selection/baseline/manual refresh, pause/resume, platform filters, actual source links, local playback and mobile layout.');
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
