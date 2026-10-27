<script lang="ts">
	import { onMount } from 'svelte';
	import api from '$lib/api';
	import { toast } from 'svelte-sonner';
	import { douyinMessage } from '$lib/platform';
	import type { Platform, ApiError, DouyinStatus, TaskStatus } from '$lib/types';
	import { Card, CardContent, CardHeader, CardTitle } from '$lib/components/ui/card/index.js';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';
	import { Input } from '$lib/components/ui/input/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	import CloudDownloadIcon from '@lucide/svelte/icons/cloud-download';
	import PlayIcon from '@lucide/svelte/icons/play';
	import CircleCheckBigIcon from '@lucide/svelte/icons/circle-check-big';
	import CalendarIcon from '@lucide/svelte/icons/calendar';
	import DownloadIcon from '@lucide/svelte/icons/download';
	let { platform }: { platform: Platform } = $props();
	let taskStatus = $state<TaskStatus | null>(null);
	let triggering = $state(false);
	let intervalInput = $state('');
	let intervalLoaded = $state(false);
	let intervalError = $state('');
	let intervalSaving = $state(false);
	let douyin = $state<DouyinStatus | null>(null);
	let douyinError = $state('');
	let douyinBusy = $state(false);

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

	async function handleTriggerDownload() {
		triggering = true;
		try {
			await api.triggerDownloadTask();
			toast.success('已触发下载任务', {
				description: '任务将立即开始执行'
			});
		} catch (error) {
			console.error('触发下载任务失败：', error);
			toast.error('触发下载任务失败', {
				description: (error as ApiError).message
			});
		} finally {
			triggering = false;
		}
	}

	async function loadInterval() {
		try {
			intervalInput = String(
				platform === 'douyin'
					? (await api.getDouyinStatus()).data.interval
					: (await api.getConfig()).data.interval
			);
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
			if (platform === 'douyin') {
				const seconds = Number(value);
				if (!Number.isSafeInteger(seconds) || seconds < 61 || seconds > 31536000) {
					toast.error('抖音同步间隔须为 61 至 31536000 的整数秒');
					return;
				}
				douyin = (await api.updateDouyinConfig(seconds)).data;
			} else {
				const config = (await api.getConfig()).data;
				config.interval = Number.isFinite(Number(value)) ? Number(value) : value;
				await api.updateConfig(config);
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
		if (platform === 'douyin') {
			void loadDouyin();
			const poll = setInterval(loadDouyin, 5000);
			return () => clearInterval(poll);
		}
		return api.subscribeToTasks((data) => {
			taskStatus = data;
		});
	});
</script>

<section class="space-y-3 rounded-lg border p-4" aria-label="同步计划">
	<h2 class="font-semibold">同步计划</h2>
	<p class="text-muted-foreground text-sm">
		此处只调整{platform === 'douyin' ? '抖音' : 'B站'}的运行频率。
	</p>
	{#if intervalLoaded}
		<div class="flex flex-wrap items-end gap-3">
			<div class="space-y-2">
				<Label for="platform-interval"
					>{platform === 'douyin' ? '抖音同步间隔（秒）' : 'B站同步间隔（秒或 Cron）'}</Label
				>
				<Input id="platform-interval" class="w-full sm:w-72" bind:value={intervalInput} />
			</div>
			<Button variant="outline" onclick={saveInterval} disabled={intervalSaving}
				>{intervalSaving ? '保存中…' : '保存同步间隔'}</Button
			>
		</div>
		<p class="text-muted-foreground text-xs">
			{platform === 'douyin'
				? '默认 1800 秒（30 分钟）。'
				: '输入秒数，例如 1200 表示 20 分钟；或输入六段 Cron，例如 0 0 2 * * * 表示每天凌晨 2 点。'}
		</p>
	{:else if intervalError}<p role="alert">{intervalError}</p>
		<Button variant="outline" onclick={loadInterval}>重试加载间隔</Button>
	{:else}<p role="status">正在加载同步间隔…</p>{/if}
</section>
{#if platform === 'douyin'}
	<section class="bg-card space-y-4 rounded-lg border p-4" aria-label="抖音任务">
		<div class="flex flex-wrap items-center justify-between gap-3">
			<h2 class="text-sm font-semibold">抖音同步任务</h2>
			{#if douyin}<Badge variant={douyin.task.is_running ? 'default' : 'outline'}
					>{douyin.task.is_running
						? '运行中'
						: douyin.auth_state === 'paused'
							? '已暂停'
							: douyin.connected
								? '等待运行'
								: '未连接'}</Badge
				>{/if}
		</div>
		{#if douyinError}<p class="text-destructive text-sm" role="alert">
				状态加载失败：{douyinError}
			</p>
			<Button variant="outline" size="sm" onclick={loadDouyin}>重试</Button>{/if}
		{#if douyin}
			<div class="grid gap-3 text-sm md:grid-cols-3">
				<div>
					<span class="text-muted-foreground">开始运行：</span>{douyin.task.last_run
						? new Date(douyin.task.last_run).toLocaleString('zh-CN')
						: '—'}
				</div>
				<div>
					<span class="text-muted-foreground">运行结束：</span>{douyin.task.last_finish
						? new Date(douyin.task.last_finish).toLocaleString('zh-CN')
						: '—'}
				</div>
				<div>
					<span class="text-muted-foreground">下次运行：</span>{douyin.task.next_run
						? new Date(douyin.task.next_run).toLocaleString('zh-CN')
						: '—'}
				</div>
			</div>
			{#if douyin.task.last_error}<p class="text-destructive text-sm" role="status">
					最近失败：{douyinMessage(douyin.task.last_error)}
				</p>{/if}
			{#if douyin.paused_reason}<p class="text-destructive text-sm" role="status">
					暂停原因：{douyinMessage(douyin.paused_reason)}（连续拒绝 {douyin.rejections} 次）
				</p>{/if}
			<div class="flex flex-wrap items-center gap-3">
				{#if douyin.auth_state === 'paused'}<Button
						size="sm"
						onclick={() => runDouyin(true)}
						disabled={douyinBusy || douyin.task.is_running}
						>{douyinBusy ? '验证中…' : '验证连接并恢复'}</Button
					>
					<p class="text-muted-foreground text-xs">
						恢复前验证同一账号。登录已失效时，请先在“连接账号”重新登录。
					</p>
				{:else if douyin.connected}<Button
						size="sm"
						onclick={() => runDouyin()}
						disabled={douyinBusy || douyin.task.is_running}
						>{douyinBusy
							? '触发中…'
							: douyin.task.is_running
								? '任务运行中'
								: douyin.task.last_error
									? '重试抖音同步'
									: '立即同步抖音'}</Button
					>
				{:else}<Button size="sm" variant="outline" href="/platforms/douyin?view=account"
						>连接抖音</Button
					>{/if}
			</div>
		{:else if !douyinError}<p class="text-muted-foreground text-sm">正在加载任务状态…</p>{/if}
	</section>{:else}
	<Card class="max-w-full md:col-span-1">
		<CardHeader class="flex flex-row items-center justify-between space-y-0 pb-2">
			<CardTitle class="text-sm font-medium">B站同步任务</CardTitle>
			<CloudDownloadIcon class="text-muted-foreground h-4 w-4" />
		</CardHeader>
		<CardContent>
			{#if taskStatus}
				<div class="space-y-4">
					<h3 class="text-sm font-semibold">B站</h3>
					<div class="grid grid-cols-1 gap-6">
						<div class="mb-4 space-y-2">
							<div class="flex items-center justify-between text-sm">
								<span>当前任务状态</span>
								<Badge variant={taskStatus.is_running ? 'default' : 'outline'}>
									{taskStatus.is_running ? '运行中' : '未运行'}
								</Badge>
							</div>
						</div>
						<div class="flex items-center justify-between">
							<div class="flex items-center gap-2">
								<PlayIcon class="text-muted-foreground h-4 w-4" />
								<span class="text-sm">开始运行</span>
							</div>
							<span class="text-muted-foreground text-sm">
								{taskStatus.last_run
									? new Date(taskStatus.last_run).toLocaleString('en-US', {
											month: '2-digit',
											day: '2-digit',
											hour: '2-digit',
											minute: '2-digit',
											second: '2-digit',
											hour12: true
										})
									: '-'}
							</span>
						</div>
						<div class="flex items-center justify-between">
							<div class="flex items-center gap-2">
								<CircleCheckBigIcon class="text-muted-foreground h-4 w-4" />
								<span class="text-sm">运行结束</span>
							</div>
							<span class="text-muted-foreground text-sm">
								{taskStatus.last_finish
									? new Date(taskStatus.last_finish).toLocaleString('en-US', {
											month: '2-digit',
											day: '2-digit',
											hour: '2-digit',
											minute: '2-digit',
											second: '2-digit',
											hour12: true
										})
									: '-'}
							</span>
						</div>
						<div class="flex items-center justify-between">
							<div class="flex items-center gap-2">
								<CalendarIcon class="text-muted-foreground h-4 w-4" />
								<span class="text-sm">下次运行</span>
							</div>
							<span class="text-muted-foreground text-sm">
								{taskStatus.next_run
									? new Date(taskStatus.next_run).toLocaleString('en-US', {
											month: '2-digit',
											day: '2-digit',
											hour: '2-digit',
											minute: '2-digit',
											second: '2-digit',
											hour12: true
										})
									: '-'}
							</span>
						</div>
					</div>
					<div class="mt-6 border-t pt-4">
						<Button
							class="w-full"
							size="sm"
							onclick={handleTriggerDownload}
							disabled={triggering || (taskStatus?.is_running ?? false)}
						>
							<DownloadIcon class="h-4 w-4" />
							{triggering
								? '触发中...'
								: taskStatus?.is_running
									? '任务运行中'
									: '立即执行下载任务'}
						</Button>
					</div>
				</div>
			{:else}
				<div class="text-muted-foreground text-sm">加载中...</div>
			{/if}
		</CardContent>
	</Card>{/if}
