const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
// Requires Playwright in the test environment and npm run preview on 127.0.0.1:5173.
// Usage: node scripts/check_watch.cjs <local MP4 sample>
const { chromium, expect: playwrightExpect } = require('playwright/test');
const expect = playwrightExpect.configure({ timeout: 15000 });

(async () => {
	const browser = await chromium.launch({ channel: 'msedge', headless: true });
	try {
		const context = await browser.newContext({
			viewport: { width: 1440, height: 1000 }
		});
		const page = await context.newPage();
		const failures = [];
		page.on('pageerror', (error) => {
			failures.push(error.message);
			console.error('BROWSER:', error.message);
		});
		assert.ok(
			process.argv[2],
			'Provide a local MP4 sample: node scripts/check_watch.cjs <sample.mp4>'
		);
		const media = await fs.readFile(process.argv[2]);
		const outputDir = path.resolve(__dirname, '../local-data/watch-smoke');
		await fs.mkdir(outputDir, { recursive: true });
		const video = (id) => ({
			id,
			name: `测试视频 ${id}`,
			bvid: `BV${id}`,
			upper_name: '测试作者',
			valid: true,
			should_download: true,
			download_status: [7, 7, 7, 7, 7]
		});
		const parts = [101, 102, 103].map((id, i) => ({
			id,
			pid: i + 1,
			name: `选集 ${i + 1}`,
			duration: 120,
			download_status: [1, i === 2 ? 0 : 7, 1, 1, 1]
		}));
		const progress = new Map([
			[102, { page_id: 102, position_ms: 2000, completed: false, revision: 1 }]
		]);
		const listRequests = [];
		let conflict = false;
		await context.routeWebSocket('**/api/ws', (socket) => {
			socket.onMessage(() => {});
		});
		await context.route('**/api/**', async (route) => {
			const req = route.request();
			const url = new URL(req.url());
			const json = (data, status = 200) =>
				route.fulfill({ status, json: { status_code: status, data } });
			if (url.pathname.startsWith('/api/media/pages/')) {
				const range = req.headers().range;
				if (range) {
					const [, first, last] = /bytes=(\d+)-(\d*)/.exec(range);
					const start = Number(first);
					const end = Math.min(last ? Number(last) : media.length - 1, media.length - 1);
					return route.fulfill({
						status: 206,
						contentType: 'video/mp4',
						headers: {
							'Content-Range': `bytes ${start}-${end}/${media.length}`,
							'Accept-Ranges': 'bytes'
						},
						body: media.subarray(start, end + 1)
					});
				}
				return route.fulfill({ contentType: 'video/mp4', body: media });
			}
			if (url.pathname === '/api/media-session') return json(null);
			if (url.pathname === '/api/continue-watching')
				return json(
					[...progress.values()]
						.filter((p) => !p.completed && p.position_ms > 0)
						.map((p) => ({
							...p,
							video_id: 1,
							video_name: '测试视频 1',
							page_name: '选集 2',
							pid: 2,
							duration: 120
						}))
				);
			if (url.pathname === '/api/videos') {
				listRequests.push(url.searchParams);
				const query = url.searchParams.get('query');
				if (query === 'error')
					return route.fulfill({
						status: 500,
						json: { message: '测试请求失败' }
					});
				if (query === 'slow') await new Promise((resolve) => setTimeout(resolve, 700));
				const ids =
					query === 'empty'
						? []
						: query
							? [query === 'slow' ? 2 : 1]
							: url.searchParams.get('page') === '1'
								? [21]
								: Array.from({ length: 20 }, (_, i) => i + 1);
				return json({
					videos: ids.map(video),
					total_count: query ? ids.length : 21
				});
			}
			if (/\/api\/videos\/\d+$/.test(url.pathname)) {
				if (url.pathname.endsWith('/22')) await new Promise((resolve) => setTimeout(resolve, 700));
				return json({
					video: video(Number(url.pathname.split('/').pop())),
					pages: parts
				});
			}
			if (url.pathname.startsWith('/api/watch-progress/')) {
				const id = Number(url.pathname.split('/').pop());
				if (req.method() === 'PUT') {
					if (conflict)
						return route.fulfill({
							status: 409,
							json: { message: 'Conflict' }
						});
					progress.set(id, {
						page_id: id,
						...req.postDataJSON(),
						revision: (progress.get(id)?.revision ?? 0) + 1
					});
				}
				return json(progress.get(id) ?? null);
			}
			if (url.pathname === '/api/dashboard')
				return json({
					enabled_favorites: 1,
					enabled_collections: 0,
					enabled_submissions: 0,
					enable_watch_later: false,
					videos_by_day: []
				});
			return json({});
		});
		const saved = page.getByRole('region', { name: '已保存视频' });
		const search = page.getByLabel('搜索标题或 BV 号');
		const submit = async (query) => {
			await search.fill(query);
			await page.getByRole('button', { name: '搜索', exact: true }).click();
		};
		await page.goto('http://127.0.0.1:5173/watch');
		await expect(saved.getByRole('link'))
			.toHaveCount(20)
			.catch(async (error) => {
				console.error(await page.locator('body').innerText());
				throw error;
			});
		await expect(page.locator('[data-slot="sidebar"]')).toHaveCount(0);
		await expect(page.getByRole('navigation', { name: '观看导航' })).toBeVisible();
		assert.equal(listRequests[0].get('saved_only'), 'true');
		assert.equal(listRequests[0].get('page_size'), '20');
		await page.getByRole('button', { name: '2', exact: true }).click();
		await expect(saved.getByRole('link')).toHaveCount(1);
		await expect(saved).toContainText('测试视频 21');
		await submit('empty');
		await expect(saved).toContainText('没有匹配的已保存视频');
		await submit('error');
		await expect(saved.getByRole('alert')).toContainText('测试请求失败');
		await page.getByRole('button', { name: '重试', exact: true }).click();
		await expect(saved.getByRole('alert')).toContainText('测试请求失败');
		const slow = page.waitForRequest((req) => req.url().includes('query=slow'));
		await submit('slow');
		await slow;
		await submit('fast');
		await expect(saved).toContainText('测试视频 1');
		await page.waitForTimeout(900);
		await expect(saved).not.toContainText('测试视频 2');
		await page.reload();
		const resume = page.getByRole('region', { name: '继续观看' }).getByRole('link').first();
		await expect(resume).toHaveAttribute('href', '/watch/1?part=102');
		await resume.click();
		await expect(page).toHaveURL(/\/watch\/1\?part=102$/);
		await expect(page.getByRole('heading', { name: '测试视频 1', exact: true })).toBeVisible();
		await expect(page.getByLabel('选择分 P')).toHaveValue('102');
		await expect(page.getByLabel('选择分 P').locator('option')).toHaveCount(2);
		const player = page.locator('video');
		await expect.poll(() => player.evaluate((el) => el.readyState)).toBeGreaterThanOrEqual(1);
		await expect.poll(() => player.evaluate((el) => el.currentTime)).toBeGreaterThan(1.5);
		await player.evaluate((el) => {
			el.currentTime = 3;
		});
		await expect.poll(() => progress.get(102)?.position_ms).toBeGreaterThanOrEqual(2900);
		await page.getByRole('button', { name: '标记已看', exact: true }).click();
		await expect.poll(() => progress.get(102)?.completed).toBe(true);
		await page.reload();
		await expect(page.getByRole('button', { name: '标记未看', exact: true })).toBeEnabled();
		await page.getByRole('button', { name: '标记未看', exact: true }).click();
		await expect.poll(() => progress.get(102)?.completed).toBe(false);
		await page.getByLabel('选择分 P').selectOption('101');
		await expect(player).toHaveAttribute('src', '/api/media/pages/101');
		await expect(page.getByRole('button', { name: '标记已看', exact: true })).toBeEnabled();
		await expect.poll(() => player.evaluate((el) => el.readyState)).toBeGreaterThanOrEqual(1);
		await page.waitForTimeout(150);
		conflict = true;
		await page.getByRole('button', { name: '标记已看', exact: true }).click();
		await expect(page.getByRole('alert')).toContainText('另一窗口更新了观看记录');
		conflict = false;
		await page.getByRole('button', { name: '重新加载观看记录' }).click();
		await expect(page.getByRole('alert')).toHaveCount(0);
		await page.getByRole('link', { name: '管理此视频', exact: true }).click();
		await expect(page).toHaveURL(/\/video\/1$/);
		await expect(page.locator('[data-slot="sidebar"]')).toBeVisible();
		await expect(page.locator('video')).toHaveCount(0);
		await page.getByRole('link', { name: '前往播放', exact: true }).click();
		await expect(page.locator('video')).toBeVisible();
		await page.getByRole('link', { name: '进入管理', exact: true }).click();
		await expect(page).toHaveURL('http://127.0.0.1:5173/');
		await expect(page.getByRole('heading', { name: '继续观看', exact: true })).toHaveCount(0);
		await page.getByRole('link', { name: '进入观看', exact: true }).click();
		await expect(page).toHaveURL('http://127.0.0.1:5173/watch');
		for (const url of ['/watch', '/watch/1?part=102']) {
			await page.goto(`http://127.0.0.1:5173${url}`);
			await page.setViewportSize({ width: 375, height: 812 });
			await expect(page.getByRole('navigation', { name: '观看导航' })).toBeVisible();
			await expect(page.locator('[data-slot="sidebar"]')).toHaveCount(0);
			await expect
				.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth))
				.toBe(true);
			await page.screenshot({
				path: path.join(outputDir, url === '/watch' ? 'watch-mobile.png' : 'player-mobile.png'),
				fullPage: true
			});
		}
		await page.setViewportSize({ width: 1440, height: 1000 });
		await page.goto('http://127.0.0.1:5173/watch');
		await expect(saved.getByRole('link')).toHaveCount(20);
		await page.screenshot({
			path: path.join(outputDir, 'watch-desktop.png'),
			fullPage: true
		});
		await page.goto('http://127.0.0.1:5173/watch/not-a-number');
		await expect(page.getByRole('alert')).toContainText('无效的视频 ID');
		await page.evaluate(() => {
			const link = document.createElement('a');
			link.id = 'test-switch';
			link.href = '/watch/22';
			link.textContent = '切换测试视频';
			document.body.append(link);
		});
		const slowVideo = page.waitForRequest('**/api/videos/22');
		await page.locator('#test-switch').click();
		await slowVideo;
		await page.locator('#test-switch').evaluate((link) => {
			link.href = '/watch/23';
		});
		await page.locator('#test-switch').click();
		await expect(page.getByRole('heading', { name: '测试视频 23', exact: true })).toBeVisible();
		await page.waitForTimeout(900);
		await expect(page.getByRole('heading', { name: '测试视频 23', exact: true })).toBeVisible();
		const dark = await page.locator('html').evaluate((el) => el.classList.contains('dark'));
		await page.getByRole('button', { name: /切换为/ }).click();
		await expect
			.poll(() => page.locator('html').evaluate((el) => el.classList.contains('dark')))
			.toBe(!dark);
		assert.deepEqual(failures, []);
		console.log(
			'PASS: independent navigation, saved search/pagination/errors/races, part links, playback/resume/progress/conflicts, admin return, refresh and mobile overflow.'
		);
	} finally {
		await browser.close();
	}
})().catch((error) => {
	console.error(error);
	process.exitCode = 1;
});
