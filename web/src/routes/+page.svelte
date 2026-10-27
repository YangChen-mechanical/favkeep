<script lang="ts">
	import { douyinMessage } from '$lib/platform';
	import { onMount } from 'svelte';
	import { Card, CardContent, CardHeader, CardTitle } from '$lib/components/ui/card/index.js';
	import { Progress } from '$lib/components/ui/progress/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';
	import { Button } from '$lib/components/ui/button/index.js';
	import * as Chart from '$lib/components/ui/chart/index.js';
	import MyChartTooltip from '$lib/components/custom/my-chart-tooltip.svelte';
	import { curveNatural } from 'd3-shape';
	import { BarChart, AreaChart } from 'layerchart';
	import { setBreadcrumb } from '$lib/stores/breadcrumb';
	import { toast } from 'svelte-sonner';
	import CloudDownloadIcon from '@lucide/svelte/icons/cloud-download';
	import api from '$lib/api';
	import type { DashBoardResponse, SysInfo, ApiError, TaskStatus, DouyinStatus } from '$lib/types';
	import CalendarIcon from '@lucide/svelte/icons/calendar';
	import CircleCheckBigIcon from '@lucide/svelte/icons/circle-check-big';
	import ClockIcon from '@lucide/svelte/icons/clock';
	import CpuIcon from '@lucide/svelte/icons/cpu';
	import DatabaseIcon from '@lucide/svelte/icons/database';
	import DownloadIcon from '@lucide/svelte/icons/download';
	import FolderIcon from '@lucide/svelte/icons/folder';
	import HardDriveIcon from '@lucide/svelte/icons/hard-drive';
	import HeartIcon from '@lucide/svelte/icons/heart';
	import MemoryStickIcon from '@lucide/svelte/icons/memory-stick';
	import PlayIcon from '@lucide/svelte/icons/play';
	import UserIcon from '@lucide/svelte/icons/user';
	import VideoIcon from '@lucide/svelte/icons/video';

	let dashboardData = $state<DashBoardResponse | null>(null);
	let sysInfo = $state<SysInfo | null>(null);
	let taskStatus = $state<TaskStatus | null>(null);
	let loading = $state(false);
	let triggering = $state(false);
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
	let memoryHistory = $state<Array<{ time: number; used: number; process: number }>>([]);
	let cpuHistory = $state<Array<{ time: number; used: number; process: number }>>([]);
	let unsubscribeSysInfo: (() => void) | null = null;
	let unsubscribeTasks: (() => void) | null = null;

	function formatBytes(bytes: number): string {
		if (bytes === 0) return '0 B';
		const k = 1024;
		const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
		const i = Math.floor(Math.log(bytes) / Math.log(k));
		return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
	}

	function formatCpu(cpu: number): string {
		return `${cpu.toFixed(1)}%`;
	}

	function formatTimestamp(timestamp: number): string {
		return new Date(timestamp).toLocaleString('en-US', {
			hour: '2-digit',
			minute: '2-digit',
			second: '2-digit',
			hour12: true
		});
	}

	async function loadDashboard() {
		loading = true;
		try {
			const response = await api.getDashboard();
			dashboardData = response.data;
		} catch (error) {
			console.error('加载仪表盘数据失败：', error);
			toast.error('加载仪表盘数据失败', {
				description: (error as ApiError).message
			});
		} finally {
			loading = false;
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

	const videoChartConfig = {
		videos: {
			label: '视频数量',
			color: 'var(--primary)'
		}
	} satisfies Chart.ChartConfig;

	const memoryChartConfig = {
		used: {
			label: '整体占用',
			color: 'var(--primary)'
		},
		process: {
			label: '程序占用',
			color: 'oklch(from var(--primary) calc(l * 0.6) c h)'
		}
	} satisfies Chart.ChartConfig;

	const cpuChartConfig = {
		used: {
			label: '整体占用',
			color: 'var(--primary)'
		},
		process: {
			label: '程序占用',
			color: 'oklch(from var(--primary) calc(l * 0.6) c h)'
		}
	} satisfies Chart.ChartConfig;

	function pushSysInfo(data: SysInfo) {
		memoryHistory = [
			...memoryHistory.slice(-14),
			{
				time: data.timestamp,
				used: data.used_memory,
				process: data.process_memory
			}
		];
		cpuHistory = [
			...cpuHistory.slice(-14),
			{
				time: data.timestamp,
				used: data.used_cpu,
				process: data.process_cpu
			}
		];
	}

	const diskUsagePercent = $derived(
		sysInfo ? ((sysInfo.total_disk - sysInfo.available_disk) / sysInfo.total_disk) * 100 : 0
	);

	onMount(() => {
		setBreadcrumb([{ label: '仪表盘' }]);

		unsubscribeSysInfo = api.subscribeToSysInfo((data) => {
			sysInfo = data;
			pushSysInfo(data);
		});
		unsubscribeTasks = api.subscribeToTasks((data: TaskStatus) => {
			taskStatus = data;
		});
		loadDashboard();
		void loadDouyin();
		const douyinPoll = setInterval(loadDouyin, 5000);
		return () => {
			clearInterval(douyinPoll);
			if (unsubscribeSysInfo) {
				unsubscribeSysInfo();
				unsubscribeSysInfo = null;
			}
			if (unsubscribeTasks) {
				unsubscribeTasks();
				unsubscribeTasks = null;
			}
		};
	});
</script>

<svelte:head>
	<title>仪表盘 - Bili Sync</title>
</svelte:head>

<div class="space-y-6">
	{#if loading}
		<div class="flex items-center justify-center py-12">
			<div class="text-muted-foreground">加载中...</div>
		</div>
	{:else}
		<div class="grid gap-4 md:grid-cols-3">
			<Card class="md:col-span-1">
				<CardHeader class="flex flex-row items-center justify-between space-y-0 pb-2">
					<CardTitle class="text-sm font-medium">存储空间</CardTitle>
					<HardDriveIcon class="text-muted-foreground h-4 w-4" />
				</CardHeader>
				<CardContent>
					{#if sysInfo}
						<div class="space-y-2">
							<div class="flex items-center justify-between">
								<div class="text-2xl font-bold">{formatBytes(sysInfo.available_disk)} 可用</div>
								<div class="text-muted-foreground text-sm">
									共 {formatBytes(sysInfo.total_disk)}
								</div>
							</div>
							<Progress value={diskUsagePercent} class="h-2" />
							<div class="text-muted-foreground text-xs">
								已使用 {diskUsagePercent.toFixed(1)}% 的存储空间
							</div>
						</div>
					{:else}
						<div class="text-muted-foreground text-sm">加载中...</div>
					{/if}
				</CardContent>
			</Card>
			<Card class="md:col-span-2">
				<CardHeader class="flex flex-row items-center justify-between space-y-0 pb-2">
					<CardTitle class="text-sm font-medium">当前监听</CardTitle>
					<DatabaseIcon class="text-muted-foreground h-4 w-4" />
				</CardHeader>
				<CardContent>
					{#if dashboardData}
						<div class="grid grid-cols-2 gap-4">
							<div class="flex items-center justify-between">
								<div class="flex items-center gap-2">
									<HeartIcon class="text-muted-foreground h-4 w-4" />
									<span class="text-sm">收藏夹</span>
								</div>
								<Badge variant="outline">{dashboardData.enabled_favorites}</Badge>
							</div>
							<div class="flex items-center justify-between">
								<div class="flex items-center gap-2">
									<FolderIcon class="text-muted-foreground h-4 w-4" />
									<span class="text-sm">合集 / 列表</span>
								</div>
								<Badge variant="outline">{dashboardData.enabled_collections}</Badge>
							</div>
							<div class="flex items-center justify-between">
								<div class="flex items-center gap-2">
									<UserIcon class="text-muted-foreground h-4 w-4" />
									<span class="text-sm">投稿</span>
								</div>
								<Badge variant="outline">{dashboardData.enabled_submissions}</Badge>
							</div>
							<div class="flex items-center justify-between">
								<div class="flex items-center gap-2">
									<ClockIcon class="text-muted-foreground h-4 w-4" />
									<span class="text-sm">稍后再看</span>
								</div>
								<Badge variant="outline">
									{dashboardData.enable_watch_later ? '启用' : '禁用'}
								</Badge>
							</div>
						</div>
					{:else}
						<div class="text-muted-foreground text-sm">加载中...</div>
					{/if}
				</CardContent>
			</Card>
		</div>

		<div class="grid gap-4 md:grid-cols-3">
			<Card class="max-w-full overflow-hidden md:col-span-2">
				<CardHeader class="flex flex-row items-center justify-between space-y-0 pb-2">
					<CardTitle class="text-sm font-medium">最近入库</CardTitle>
					<VideoIcon class="text-muted-foreground h-4 w-4" />
				</CardHeader>
				<CardContent>
					{#if dashboardData && dashboardData.videos_by_day.length > 0}
						<div class="mb-4 space-y-2">
							<div class="flex items-center justify-between text-sm">
								<span>近七日新增视频</span>
								<span class="font-medium"
									>{dashboardData.videos_by_day.reduce((sum, v) => sum + v.cnt, 0)} 个</span
								>
							</div>
						</div>
						<Chart.Container config={videoChartConfig} class="h-[200px] w-full">
							<BarChart
								data={dashboardData.videos_by_day}
								x="day"
								axis="x"
								series={[
									{
										key: 'cnt',
										label: '新增视频',
										color: videoChartConfig.videos.color
									}
								]}
								props={{
									bars: {
										stroke: 'none',
										rounded: 'all',
										radius: 8,
										initialHeight: 0
									},
									highlight: { area: { fill: 'none' } },
									xAxis: { format: () => '' }
								}}
							>
								{#snippet tooltip({ context })}
									<MyChartTooltip {context} indicator="line" />
								{/snippet}
							</BarChart>
						</Chart.Container>
					{:else}
						<div class="text-muted-foreground flex h-[200px] items-center justify-center text-sm">
							暂无视频统计数据
						</div>
					{/if}</CardContent
				>
			</Card>
			<Card class="max-w-full md:col-span-1">
				<CardHeader class="flex flex-row items-center justify-between space-y-0 pb-2">
					<CardTitle class="text-sm font-medium">平台下载任务</CardTitle>
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
			</Card>
		</div>

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
							恢复前验证同一账号。登录已失效时，请先在设置重新登录。
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
					{:else}<Button size="sm" variant="outline" href="/settings">连接抖音</Button>{/if}
				</div>
			{:else if !douyinError}<p class="text-muted-foreground text-sm">正在加载任务状态…</p>{/if}
		</section>

		<!-- 第三行：系统监控 -->
		<div class="grid gap-4 md:grid-cols-2">
			<!-- 内存使用情况 -->
			<Card class="overflow-hidden">
				<CardHeader class="flex flex-row items-center justify-between space-y-0 pb-2">
					<CardTitle class="text-sm font-medium">内存使用情况</CardTitle>
					<MemoryStickIcon class="text-muted-foreground h-4 w-4" />
				</CardHeader>
				<CardContent>
					{#if sysInfo}
						<div class="mb-4 space-y-2">
							<div class="flex items-center justify-between text-sm">
								<span>当前内存使用</span>
								<span class="font-medium"
									>{formatBytes(sysInfo.used_memory)} / {formatBytes(sysInfo.total_memory)}</span
								>
							</div>
						</div>
					{/if}
					{#if memoryHistory.length > 0}
						<Chart.Container config={memoryChartConfig} class="h-[150px] w-full">
							<AreaChart
								data={memoryHistory}
								x="time"
								axis="x"
								series={[
									{
										key: 'used',
										label: memoryChartConfig.used.label,
										color: memoryChartConfig.used.color
									},
									{
										key: 'process',
										label: memoryChartConfig.process.label,
										color: memoryChartConfig.process.color
									}
								]}
								props={{
									area: {
										curve: curveNatural,
										line: { class: 'stroke-1' },
										'fill-opacity': 0.4
									},
									xAxis: {
										format: () => ''
									}
								}}
							>
								{#snippet tooltip({ context })}
									<MyChartTooltip
										{context}
										labelFormatter={(timestamp: number) => {
											return formatTimestamp(timestamp);
										}}
										valueFormatter={(v: number) => formatBytes(v)}
										indicator="line"
									/>
								{/snippet}
							</AreaChart>
						</Chart.Container>
					{:else}
						<div class="text-muted-foreground flex h-[200px] items-center justify-center text-sm">
							等待数据...
						</div>
					{/if}
				</CardContent>
			</Card>

			<Card class="overflow-hidden">
				<CardHeader class="flex flex-row items-center justify-between space-y-0 pb-2">
					<CardTitle class="text-sm font-medium">CPU 使用情况</CardTitle>
					<CpuIcon class="text-muted-foreground h-4 w-4" />
				</CardHeader>
				<CardContent class="overflow-hidden">
					{#if sysInfo}
						<div class="mb-4 space-y-2">
							<div class="flex items-center justify-between text-sm">
								<span>当前 CPU 使用率</span>
								<span class="font-medium">{formatCpu(sysInfo.used_cpu)}</span>
							</div>
						</div>
					{/if}
					{#if cpuHistory.length > 0}
						<Chart.Container config={cpuChartConfig} class="h-[150px] w-full">
							<AreaChart
								data={cpuHistory}
								x="time"
								axis="x"
								series={[
									{
										key: 'used',
										label: cpuChartConfig.used.label,
										color: cpuChartConfig.used.color
									},
									{
										key: 'process',
										label: cpuChartConfig.process.label,
										color: cpuChartConfig.process.color
									}
								]}
								props={{
									area: {
										curve: curveNatural,
										line: { class: 'stroke-1' },
										'fill-opacity': 0.4
									},
									xAxis: {
										format: () => ''
									}
								}}
							>
								{#snippet tooltip({ context })}
									<MyChartTooltip
										{context}
										labelFormatter={(timestamp: number) => {
											return formatTimestamp(timestamp);
										}}
										valueFormatter={(v: number) => formatCpu(v)}
										indicator="line"
									/>
								{/snippet}
							</AreaChart>
						</Chart.Container>
					{:else}
						<div class="text-muted-foreground flex h-[150px] items-center justify-center text-sm">
							等待数据...
						</div>
					{/if}
				</CardContent>
			</Card>
		</div>
	{/if}
</div>
