<script lang="ts">
	import { onMount } from 'svelte';
	import api from '$lib/api';
	import { toast } from 'svelte-sonner';
	import { douyinMessage } from '$lib/platform';
	import type { ApiError, DouyinStatus } from '$lib/types';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';
	import { Input } from '$lib/components/ui/input/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	let intervalInput = $state('');
	let intervalLoaded = $state(false);
	let intervalError = $state('');
	let intervalSaving = $state(false);
	let douyin = $state<DouyinStatus | null>(null);
	let douyinError = $state('');
	let douyinBusy = $state(false);
	const operations: Record<string, string> = {
		login: '连接账号',
		refresh: '刷新收藏夹',
		sync: '同步下载',
		resume: '验证连接与恢复'
	};
	const stages: Record<string, string> = {
		preparing: '准备中',
		restoring: '恢复会话与验证账号',
		qr_preparing: '准备二维码',
		qr_ready: '等待扫码确认',
		folders: '获取收藏夹列表',
		scanning: '扫描收藏夹',
		downloading: '下载与校验视频',
		registering: '登记已保存视频'
	};
	const counts = [
		['scanned', '扫描条目'],
		['historical', '历史跳过'],
		['existing', '已有条目'],
		['unsupported', '不支持条目'],
		['inserted', '新增入库'],
		['downloaded', '下载成功'],
		['reused', '成品复用'],
		['recovered', '中断恢复'],
		['failed', '下载失败'],
		['pending', '剩余待处理']
	] as const;
	async function loadDouyin() {
		try {
			const status = (await api.getDouyinStatus()).data;
			if (!status?.task || typeof status.task.is_running !== 'boolean')
				throw new Error('抖音任务状态无效，请刷新重试');
			douyin = status;
			douyinError = '';
		} catch (error) {
			douyinError = (error as ApiError).message;
		}
	}
	async function runDouyin(resume = false) {
		douyinBusy = true;
		try {
			if (resume) await api.resumeDouyin();
			else await api.triggerDouyinTask();
			await loadDouyin();
			toast.success(resume ? '已验证连接并恢复抖音任务' : '已触发抖音同步');
		} catch (error) {
			toast.error(resume ? '恢复抖音任务失败' : '触发抖音同步失败', {
				description: (error as ApiError).message
			});
		} finally {
			douyinBusy = false;
		}
	}
	async function loadInterval() {
		try {
			intervalInput = String((await api.getDouyinStatus()).data.interval);
			intervalLoaded = true;
			intervalError = '';
		} catch (error) {
			intervalError = (error as ApiError).message;
		}
	}
	async function saveInterval() {
		const value = intervalInput.trim();
		if (!value) {
			toast.error('请输入同步间隔');
			return;
		}
		intervalSaving = true;
		try {
			{
				const seconds = Number(value);
				if (!Number.isSafeInteger(seconds) || seconds < 61 || seconds > 31536000) {
					toast.error('抖音同步间隔须为 61 至 31536000 的整数秒');
					return;
				}
				douyin = (await api.updateDouyinConfig(seconds)).data;
			}
			toast.success('同步间隔已保存');
		} catch (error) {
			toast.error('保存同步间隔失败', { description: (error as ApiError).message });
		} finally {
			intervalSaving = false;
		}
	}
	onMount(() => {
		void loadInterval();
		void loadDouyin();
		const poll = setInterval(loadDouyin, 5000);
		return () => clearInterval(poll);
	});
</script>

<section class="space-y-3 rounded-lg border p-4" aria-label="同步计划">
	<h2 class="font-semibold">同步计划</h2>
	<p class="text-muted-foreground text-sm">此处只调整抖音的运行频率。</p>

	{#if intervalLoaded}
		<div class="flex flex-wrap items-end gap-3">
			<div class="space-y-2">
				<Label for="platform-interval">抖音同步间隔（秒）</Label>

				<Input id="platform-interval" class="w-full sm:w-72" bind:value={intervalInput} />
			</div>

			<Button variant="outline" onclick={saveInterval} disabled={intervalSaving}
				>{intervalSaving ? '保存中…' : '保存同步间隔'}</Button
			>
		</div>

		<p class="text-muted-foreground text-xs">默认 1800 秒（30 分钟）。</p>
	{:else if intervalError}
		<p role="alert">{intervalError}</p>
		<Button variant="outline" onclick={loadInterval}>重试加载间隔</Button>
	{:else}
		<p role="status">正在加载同步间隔…</p>
	{/if}
</section>

<section class="bg-card space-y-4 rounded-lg border p-4" aria-label="抖音任务">
	<div class="flex flex-wrap items-center justify-between gap-3">
		<h2 class="text-sm font-semibold">抖音任务状态</h2>

		{#if douyin}
			<Badge variant={douyin.task.is_running ? 'default' : 'outline'}>
				{douyin.task.is_running
					? `${operations[douyin.task.current?.operation ?? ''] ?? '账号操作'}中`
					: douyin.auth_state === 'paused'
						? '已暂停'
						: douyin.connected
							? '等待运行'
							: '未连接'}
			</Badge>
		{/if}
	</div>

	{#if douyinError}
		<p class="text-destructive text-sm" role="alert">状态加载失败：{douyinError}</p>
		<Button variant="outline" size="sm" onclick={loadDouyin}>重试</Button>
	{/if}

	{#if douyin}
		{#if douyin.task.current}
			<div class="space-y-1 break-all rounded-md border p-3 text-sm" role="status">
				<p>当前操作：{operations[douyin.task.current.operation] ?? '账号操作'}</p>
				<p>当前阶段：{stages[douyin.task.current.stage] ?? '处理中'}</p>
				{#if douyin.task.current.folder}<p>当前收藏夹：{douyin.task.current.folder}</p>{/if}
				{#if douyin.task.current.video}<p>当前视频：{douyin.task.current.video}</p>{/if}
			</div>
		{/if}
		<div class="grid gap-3 text-sm md:grid-cols-3">
			<div>
				<span class="text-muted-foreground">开始运行：</span>

				{douyin.task.last_run ? new Date(douyin.task.last_run).toLocaleString('zh-CN') : '—'}
			</div>

			<div>
				<span class="text-muted-foreground">运行结束：</span>

				{douyin.task.last_finish ? new Date(douyin.task.last_finish).toLocaleString('zh-CN') : '—'}
			</div>

			<div>
				<span class="text-muted-foreground">下次运行：</span>

				{douyin.task.next_run ? new Date(douyin.task.next_run).toLocaleString('zh-CN') : '—'}
			</div>
		</div>
		<section class="space-y-3 rounded-md border p-3" aria-label="本轮同步统计">
			{#if douyin.task.last_sync}
				<h3 class="text-sm font-medium">
					{douyin.task.last_sync.finished_at ? '最近一轮同步' : '本轮同步'}
				</h3>
				<p class="text-muted-foreground text-xs">
					开始于 {new Date(douyin.task.last_sync.started_at).toLocaleString('zh-CN')} ·
					{douyin.task.last_sync.finished_at
						? douyin.task.last_sync.error
							? '未完成，保留已完成部分'
							: '已完成'
						: '运行中'}
				</p>
				<dl class="grid grid-cols-2 gap-3 text-sm sm:grid-cols-3 lg:grid-cols-5">
					{#each counts as [key, label] (key)}
						<div>
							<dt class="text-muted-foreground">{label}</dt>
							<dd class="mt-1 font-medium">{douyin.task.last_sync[key] ?? '未知'}</dd>
						</div>
					{/each}
				</dl>
				{#if douyin.task.last_sync.error}
					<p class="text-destructive break-words text-sm" role="status">
						本轮结果：{douyin.task.last_sync.error === 'cancelled'
							? '任务已取消或中断，请重新同步'
							: douyinMessage(douyin.task.last_sync.error)}
					</p>
				{:else if douyin.task.last_sync.finished_at && douyin.task.last_sync.downloaded === 0}
					<p class="text-muted-foreground text-xs">
						本轮没有新下载。首次基线跳过历史；已有、不支持及复用或恢复的文件不计入新下载。详情见日志。
					</p>
				{/if}
				<p class="text-muted-foreground text-xs">
					扫描条目按收藏夹累计，跨夹重复仅新增入库一次。剩余待处理仅含当前来源允许且未达到重试上限的条目。
				</p>
			{:else}
				<p class="text-muted-foreground text-sm">暂无本次运行统计</p>
			{/if}
		</section>

		{#if douyin.task.last_error}
			<p class="text-destructive text-sm" role="status">
				最近失败：{douyinMessage(douyin.task.last_error)}
			</p>
		{/if}

		{#if douyin.paused_reason}
			<p class="text-destructive text-sm" role="status">
				暂停原因：{douyinMessage(douyin.paused_reason)}（连续拒绝 {douyin.rejections} 次）
			</p>
		{/if}

		<div class="flex flex-wrap items-center gap-3">
			<Button size="sm" variant="outline" href="/logs">查看日志</Button>
			<Button size="sm" variant="outline" href="/watch?platform=douyin">查看已保存视频</Button>
			{#if douyin.auth_state === 'paused'}
				<Button
					size="sm"
					onclick={() => runDouyin(true)}
					disabled={douyinBusy || douyin.task.is_running}
					>{douyinBusy ? '验证中…' : '验证连接并恢复'}</Button
				>

				<p class="text-muted-foreground text-xs">
					恢复前验证同一账号。登录已失效时，请先在“连接账号”重新登录。
				</p>
			{:else if douyin.connected}
				<Button
					size="sm"
					onclick={() => runDouyin()}
					disabled={douyinBusy || douyin.task.is_running}
				>
					{douyinBusy
						? '触发中…'
						: douyin.task.is_running
							? '任务运行中'
							: douyin.task.last_error
								? '重试抖音同步'
								: '立即同步抖音'}
				</Button>
			{:else}
				<Button size="sm" variant="outline" href="/platforms/douyin?view=account">连接抖音</Button>
			{/if}
		</div>
	{:else if !douyinError}
		<p class="text-muted-foreground text-sm">正在加载任务状态…</p>
	{/if}
</section>
