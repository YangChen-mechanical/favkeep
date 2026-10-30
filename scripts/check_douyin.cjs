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
      task: { is_running: false, current: null, last_sync: null, last_run: '2026-10-26T12:00:00Z', last_finish: '2026-10-26T12:01:00Z', next_run: '2026-10-26T12:31:00Z', last_error: null }
    };
    const folders = [
      { id: '111', name: '抖音精选', enabled: true, baseline_at: '2026-10-25T12:00:00Z', last_sync_at: '2026-10-26T12:01:00Z', last_error: null, video_count: 3 },
      { id: '222', name: '稍后慢慢看', enabled: false, baseline_at: null, last_sync_at: null, last_error: null, video_count: 0 },
      { id: '333', name: '失效收藏夹', enabled: false, available: false, baseline_at: null, last_sync_at: null, last_error: null, video_count: 0 }
    ];
    const videos = [
      { id: 1, platform: 'bilibili', platform_id: 'BVfixture', bvid: 'BVfixture', name: 'B站测试视频', upper_name: 'B站作者' },
      { id: 2, platform: 'douyin', platform_id: '1234567890123456789', bvid: null, name: '抖音测试视频', upper_name: '抖音作者' }
    ].map(video => ({ ...video, valid: true, should_download: true, download_state: 'succeeded', media_ready: true, source_url: video.platform === 'bilibili' ? 'https://www.bilibili.com/video/BVfixture/' : 'https://www.douyin.com/video/1234567890123456789', allowed_actions: video.platform === 'bilibili' ? ['status_edit','retry_failed','force_retry','clear_redownload'] : [], platform_details: video.platform === 'bilibili' ? {platform:'bilibili',download_status:[7,7,7,7,7]} : {platform:'douyin',failures:0} }));
    let refreshes = 0;
    let qrGenerations = 0;
    let qrPolls = 0;
    let qrMode = 'pending';
    let resumes = 0;
    let runs = 0;
    let progress = null;
    let biliRuns = 0;
    let douyinReads = 0;
    let douyinConfigWrites = 0;
    const changes = [];
    const queries = [];
    const qrImage = 'data:image/svg+xml;base64,' + Buffer.from('<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><rect width="100" height="100" fill="white"/></svg>').toString('base64');
    await context.routeWebSocket('**/api/ws', socket => socket.onMessage(message => {
      if (JSON.parse(message).subscribe === 'tasks') socket.send(JSON.stringify({ tasks: { is_running: false, last_run: null, last_finish: null, next_run: '2026-10-26T12:31:00Z' } }));
    }));
    await context.route('**/api/**', async route => {
      const request = route.request();
      const url = new URL(request.url());
      const json = data => route.fulfill({ json: { status_code: 200, data } });
      if (request.method() !== 'GET') {
        assert.equal(request.headers()['x-csrf-token'], 'douyin-smoke-csrf');
        assert.equal(request.headers().origin, origin);
      }
      if (url.pathname === '/api/platforms') return json([{ id: 'bilibili', name: 'B站', capabilities: ['status_edit','force_retry','clear_redownload','danmaku','subtitles','multipart','download_settings','qr_refresh'] }, { id: 'douyin', name: '抖音', capabilities: ['retry_failed'] }]);
      if (url.pathname === '/api/auth/session') return json({ setup_required: false, authenticated: true, username: 'smoke', csrf_token: 'douyin-smoke-csrf' });
      if (url.pathname === '/api/config') {
        if (request.method() === 'PUT') Object.assign(config, request.postDataJSON(), { version: config.version + 1 });
        return json(config);
      }
      if (url.pathname === '/api/config/notifiers') return json({ version: config.version, items: [] });
      if (url.pathname === '/api/douyin/status') { douyinReads++; return json(status); }
      if (url.pathname === '/api/config/douyin') {
        douyinConfigWrites++; status.interval = request.postDataJSON().interval;
        assert.equal(config.interval, 1200, 'Douyin interval must not alter Bili interval');
        config.version++;
        return json(status);
      }
      if (url.pathname === '/api/login/qrcode/generate') {
        assert.equal(url.searchParams.get('platform'), 'douyin');
        assert.equal(url.searchParams.get('force_qr'), 'true', 'Explicit login must request a fresh QR even when a saved session exists');
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
      if (url.pathname === '/api/task/douyin') {
        runs++; status.task.is_running = true;
        status.task.current = { operation: 'sync', stage: 'downloading', folder: null, video: '1234567890123456789' };
        status.task.last_sync = { started_at: '2026-10-30T03:00:00Z', finished_at: null, scanned: 9, historical: 3, existing: 2, unsupported: 1, inserted: 3, downloaded: 1, reused: 1, recovered: 0, failed: 1, pending: null, error: null };
        return json(true);
      }
      if (url.pathname === '/api/task/download') { biliRuns++; return json(true); }
      if (url.pathname === '/api/dashboard') return json({ enabled_favorites: 0, enabled_collections: 0, enabled_submissions: 0, enable_watch_later: false, videos_by_day: [] });
      if (url.pathname === '/api/videos') {
        queries.push(url.searchParams);
        const selected = videos.filter(video => !url.searchParams.get('platform') || video.platform === url.searchParams.get('platform'));
        return json({ videos: selected, total_count: selected.length, watch_summaries: selected.map(video => ({ video_id: video.id, duration: 120, page_count: 1, watch_state: 'unwatched', resume_page_id: 200 + video.id, resume_position_ms: 0 })) });
      }
      if (/^\/api\/videos\/\d+$/.test(url.pathname)) {
        const video = videos.find(item => item.id === Number(url.pathname.split('/').pop()));
        return json({ video, pages: [{ id: 200 + video.id, pid: 1, name: video.name, duration: 120, download_state:'succeeded',media_ready:true,source_url:video.source_url,allowed_actions:[],platform_details:video.platform_details }], playable_page_ids: [200 + video.id], watch_progress: [], watch_summary: null });
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
    await expect(page.getByRole('region', { name: '访问与密码', exact: true })).toBeVisible();
    await expect(page.getByRole('tab', { name: '平台连接', exact: true })).toHaveCount(0);
    await expect(page.getByRole('tab', { name: 'B站视频处理', exact: true })).toHaveCount(0);
    await expect(page.getByRole('button', { name: '连接抖音', exact: true })).toHaveCount(0);
    await page.locator('a[data-sidebar="menu-button"][href="/platforms/douyin"]').click();
    await expect(page).toHaveURL(origin + '/platforms/douyin');
    await expect(page.locator('a[data-sidebar="menu-button"][href="/platforms/douyin"]')).toHaveAttribute('aria-current', 'page');
    const connection = page.getByRole('region', { name: '抖音连接', exact: true });
    await expect(connection).toContainText('已连接');
    assert.equal(qrGenerations, 0, 'Opening a connected platform must not start login');
    await page.getByRole('tab', { name: '3. 同步任务', exact: true }).click();
    await expect(page).toHaveURL(origin + '/platforms/douyin?view=sync');
    await expect(page.getByText('暂无本次运行统计', { exact: true })).toBeVisible();
    await page.getByLabel('抖音同步间隔（秒）').fill('60');
    await page.getByRole('button', { name: '保存同步间隔' }).click();
    await expect(page.getByText('抖音同步间隔须为 61 至 31536000 的整数秒', {exact: true})).toBeVisible();
    assert.equal(douyinConfigWrites, 0);
    await page.getByLabel('抖音同步间隔（秒）').fill('1801');
    await page.getByRole('button', { name: '保存同步间隔' }).click();
    await expect.poll(() => status.interval).toBe(1801);
    await page.reload();
    await expect(page.getByRole('tab', { name: '3. 同步任务', exact: true })).toHaveAttribute('aria-selected', 'true');
    await expect(page.getByLabel('抖音同步间隔（秒）')).toHaveValue('1801');
    await page.getByRole('tab', { name: '1. 连接账号', exact: true }).click();
    await connection.getByRole('button', { name: '重新登录', exact: true }).click();
    await expect.poll(() => qrGenerations).toBe(1);
    await expect.poll(() => qrPolls).toBeGreaterThan(0);
    await expect(page.getByRole('img', { name: '登录二维码', exact: true })).toHaveAttribute('src', qrImage);
    await expect(page.getByRole('dialog')).toBeVisible();
    qrMode = 'success';
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

    await page.getByRole('link', { name: '下一步：选择收藏来源', exact: true }).click();
    await expect(page).toHaveURL(origin + '/platforms/douyin?view=sources');
    await page.goBack();
    await expect(connection).toBeVisible();
    await page.goForward();
    await expect(page.getByRole('region', { name: '抖音收藏夹', exact: true })).toBeVisible();
    await page.reload();
    await expect(page.getByRole('tab', { name: '2. 收藏来源', exact: true })).toHaveAttribute('aria-selected', 'true');
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
    await expect(page.getByText('已启用，沿用已有基线，恢复新增视频同步', { exact: true })).toBeVisible();
    await expect(page.getByText('已启用，将补齐此收藏夹的视频', { exact: true })).toHaveCount(0);
    assert.deepEqual(changes, [{ id: '111', enabled: false }, { id: '111', enabled: true }]);
    assert.equal(refreshes, 1, 'Viewing local folders must not contact the platform');
    await page.getByRole('button', { name: '刷新抖音收藏夹', exact: true }).click();
    await expect.poll(() => refreshes).toBe(2);

    await page.screenshot({ path: path.join(output, 'douyin-sources-desktop.png'), fullPage: true });
    const readsBeforeBili = douyinReads;
    await page.locator('a[data-sidebar="menu-button"][href="/platforms/bilibili"]').click();
    await expect(page.getByRole('region', { name: 'B站连接', exact: true })).toContainText('B 站凭据已配置');
    await page.getByRole('link', { name: '下一步：选择收藏来源', exact: true }).click();
    await expect(page.getByRole('tab', { name: '收藏夹', exact: true })).toBeVisible();
    await expect(page.getByRole('tab', { name: '抖音收藏夹', exact: true })).toHaveCount(0);
    await expect(page.getByRole('link', { name: '从我的收藏夹添加', exact: true })).toHaveAttribute('href', '/me/favorites');
    await page.getByRole('link', { name: '下一步：查看同步任务', exact: true }).click();
    await expect(page.getByRole('button', { name: '立即执行下载任务', exact: true })).toBeVisible();
    await page.getByLabel('B站同步间隔（秒或 Cron）').fill('2400');
    await page.getByRole('button', { name: '保存同步间隔', exact: true }).click();
    await expect.poll(() => config.interval).toBe(2400);
    assert.equal(status.interval, 1801, 'Bili settings must preserve Douyin interval');
    await page.getByRole('button', { name: '立即执行下载任务', exact: true }).click();
    await expect.poll(() => biliRuns).toBe(1);
    await page.getByRole('tab', { name: '4. 下载设置', exact: true }).click();
    await expect(page.getByLabel('视频名称模板', { exact: true })).toHaveValue('title');
    await page.getByLabel('视频名称模板', { exact: true }).fill('changed title');
    await page.getByRole('button', { name: '保存 B站下载设置', exact: true }).click();
    await expect.poll(() => config.video_name).toBe('changed title');
    assert.equal(config.interval, 2400);
    assert.equal(status.interval, 1801);
    await page.screenshot({ path: path.join(output, 'bilibili-settings-desktop.png'), fullPage: true });
    await page.getByRole('tab', { name: '弹幕渲染', exact: true }).click();
    await expect(page.getByRole('button', { name: '保存 B站下载设置', exact: true })).toBeVisible();
    assert.equal(douyinReads, readsBeforeBili, 'Bili pages must not poll Douyin');
    await page.goto(origin + '/video-sources');
    await expect(page).toHaveURL(origin + '/platforms/bilibili?view=sources');
    await page.goto(origin + '/video-sources?platform=douyin');
    await expect(page).toHaveURL(origin + '/platforms/douyin?view=sources');

    for (const [operation, stage, label, detail] of [
      ['login', 'qr_ready', '连接账号', '等待扫码确认'],
      ['refresh', 'folders', '刷新收藏夹', '获取收藏夹列表']
    ]) {
      status.task.is_running = true;
      status.task.current = { operation, stage, folder: null, video: null };
      await page.goto(origin + '/platforms/douyin?view=sync');
      await expect(page.getByRole('region', { name: '抖音任务', exact: true })).toContainText(`当前操作：${label}`);
      await expect(page.getByRole('region', { name: '抖音任务', exact: true })).toContainText(`当前阶段：${detail}`);
      await expect(page.getByText('暂无本次运行统计', { exact: true })).toBeVisible();
    }
    status.task.is_running = false; status.task.current = null;
    status.auth_state = 'paused'; status.paused_reason = 'rejected'; status.rejections = 3; status.task.last_error = 'rejected'; status.task.next_run = null;
    await page.goto(origin + '/platforms/douyin?view=sync');
    const task = page.getByRole('region', { name: '抖音任务', exact: true });
    await expect(task).toContainText('连续拒绝 3 次');
    await expect(task.getByRole('button', { name: '立即同步抖音', exact: true })).toHaveCount(0);
    await task.getByRole('button', { name: '验证连接并恢复', exact: true }).click();
    await expect.poll(() => resumes).toBe(1);
    await task.getByRole('button', { name: '立即同步抖音', exact: true }).click();
    await expect.poll(() => runs).toBe(1);
    await expect(task.getByRole('button', { name: '任务运行中', exact: true })).toBeDisabled();
    await expect(task).toContainText('当前操作：同步下载');
    await expect(task).toContainText('当前视频：1234567890123456789');
    const summary = page.getByRole('region', { name: '本轮同步统计', exact: true });
    await expect(summary.getByText('下载成功', { exact: true }).locator('..').locator('dd')).toHaveText('1');
    await expect(summary.getByText('剩余待处理', { exact: true }).locator('..').locator('dd')).toHaveText('未知');
    status.task.is_running = false; status.task.current = null;
    status.task.last_sync.finished_at = '2026-10-30T03:01:00Z';
    status.task.last_sync.error = 'network_failed';
    await page.reload();
    await expect(summary).toContainText('未完成，保留已完成部分');
    await expect(summary).toContainText('网络连接失败');
    await expect(summary.getByText('下载成功', { exact: true }).locator('..').locator('dd')).toHaveText('1');
    await expect(task.getByText('当前操作：同步下载', { exact: true })).toHaveCount(0);
    await page.screenshot({ path: path.join(output, 'douyin-task-summary-desktop.png'), fullPage: true });
    status.task.last_sync.error = null;
    status.task.last_sync.downloaded = 0;
    status.task.last_sync.pending = 0;
    await page.reload();
    await expect(summary).toContainText('本轮没有新下载');

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
      for (const route of ['/settings', '/platforms/douyin', '/platforms/douyin?view=sources', '/platforms/douyin?view=sync', '/platforms/bilibili', '/platforms/bilibili?view=sources', '/platforms/bilibili?view=sync', '/platforms/bilibili?view=settings', '/', '/watch?platform=douyin', '/watch/2', '/video/2']) {
        await page.goto(origin + route);
        if (route.startsWith('/platforms/')) {
          await expect(page.getByRole('heading', { name: route.includes('douyin') ? '抖音' : 'B站', exact: true, level: 1 })).toBeVisible();
          if (route.includes('view=sources')) await expect(page.getByText(route.includes('douyin') ? '抖音精选' : '暂无收藏夹', { exact: true })).toBeVisible();
          if (route.includes('view=sync')) await expect(page.getByRole('region', { name: '同步计划', exact: true })).toContainText('同步间隔');
        }
        await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), { message: `Horizontal overflow at ${route}, width ${width}` }).toBe(true);
        if (width === 390) await page.screenshot({ path: path.join(output, `${route.replaceAll('/', '-').replaceAll('?', '-').replaceAll('=', '-') || 'dashboard'}-mobile.png`), fullPage: true });
      }
    }
    const generationsBeforeUnknown = qrGenerations;
    await page.goto(origin + '/platforms/unknown');
    await expect(page.getByRole('alert')).toContainText('不支持此平台');
    assert.equal(qrGenerations, generationsBeforeUnknown);
    await page.goto(origin + '/video-sources?platform=unknown');
    await expect(page.getByRole('alert')).toContainText('不支持此平台');
    assert.equal(qrGenerations, generationsBeforeUnknown);
    assert.deepEqual(errors, []);
    console.log('PASS: Platform navigation/deep links/back/reload, Bili isolation/settings/task, general settings, Douyin QR pending image/reuse/error, distinct operation stages, absent/partial/zero summaries, interval isolation, folder selection/baseline/manual refresh, pause/resume, platform filters, actual source links, local playback and mobile layout.');
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
