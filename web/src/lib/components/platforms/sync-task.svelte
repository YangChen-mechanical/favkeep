<script lang="ts">
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { createSyncTask, type SyncActions } from '$lib/platforms/sync-task';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';
	import { Input } from '$lib/components/ui/input/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	let {
		name,
		platform,
		getStatus,
		updateInterval,
		trigger,
		resume,
		message,
		stages,
		summaryNote,
		insertedLabel,
		syncLabel,
		retryLabel
	}: SyncActions & {
		platform: 'douyin' | 'xiaohongshu';
		message: (code: string) => string;
		stages: Record<string, string>;
		summaryNote: string;
		insertedLabel: string;
		syncLabel: string;
		retryLabel: string;
	} = $props();
	const task = createSyncTask({
		get name() {
			return name;
		},
		getStatus: () => getStatus(),
		updateInterval: (seconds) => updateInterval(seconds),
		trigger: () => trigger(),
		resume: () => resume()
	});
	const status = $derived($task.status);
	const cooldown = $derived(status && 'next_allowed_at' in status ? status.next_allowed_at : null);
	const windowSummary = $derived(
		status?.task.last_sync && 'window_size' in status.task.last_sync ? status.task.last_sync : null
	);
	const operations: Record<string, string> = {
		login: '连接账号',
		refresh: '刷新收藏夹',
		sync: '同步下载',
		resume: '验证连接与恢复'
	};
	const counts = $derived([
		['scanned', '扫描条目'],
		['historical', '历史跳过'],
		['existing', '已有条目'],
		['unsupported', '不支持条目'],
		['inserted', insertedLabel],
		['downloaded', '下载成功'],
		['reused', '成品复用'],
		['recovered', '中断恢复'],
		['failed', '下载失败'],
		['pending', '剩余待处理']
	] as const);
	async function saveInterval() {
		try {
			await task.saveInterval($task.interval);
			toast.success('同步间隔已保存');
		} catch (error) {
			toast.error('保存同步间隔失败', { description: (error as Error).message });
		}
	}
	async function run(resume = false) {
		try {
			await task.run(resume);
			toast.success(resume ? `已验证连接并恢复${name}任务` : `已触发${name}同步`);
		} catch (error) {
			toast.error(resume ? `恢复${name}任务失败` : `触发${name}同步失败`, {
				description: (error as Error).message
			});
		}
	}
	onMount(() => task.start());
</script>

<section class="space-y-3 rounded-lg border p-4" aria-label="同步计划">
	<h2 class="font-semibold">同步计划</h2>
	<p class="text-muted-foreground text-sm">此处只调整{name}的运行频率。</p>

	{#if $task.intervalLoaded}
		<div class="flex flex-wrap items-end gap-3">
			<div class="space-y-2">
				<Label for="platform-interval">{name}同步间隔（秒）</Label>

				<Input id="platform-interval" class="w-full sm:w-72" bind:value={$task.interval} />
			</div>

			<Button variant="outline" onclick={saveInterval} disabled={$task.intervalSaving}
				>{$task.intervalSaving ? '保存中…' : '保存同步间隔'}</Button
			>
		</div>

		<p class="text-muted-foreground text-xs">默认 1800 秒（30 分钟）。</p>
	{:else if $task.intervalError}
		<p role="alert">{$task.intervalError}</p>
		<Button variant="outline" onclick={task.loadInterval}>重试加载间隔</Button>
	{:else}
		<p role="status">正在加载同步间隔…</p>
	{/if}
</section>

<section class="bg-card space-y-4 rounded-lg border p-4" aria-label="{name}任务">
	<div class="flex flex-wrap items-center justify-between gap-3">
		<h2 class="text-sm font-semibold">{name}任务状态</h2>

		{#if status}
			<Badge variant={status.task.is_running ? 'default' : 'outline'}>
				{status.task.is_running
					? `${operations[status.task.current?.operation ?? ''] ?? '账号操作'}中`
					: status.auth_state === 'paused'
						? '已暂停'
						: status.connected
							? '等待运行'
							: '未连接'}
			</Badge>
		{/if}
	</div>

	{#if $task.error}
		<p class="text-destructive text-sm" role="alert">状态加载失败：{$task.error}</p>
		<Button variant="outline" size="sm" onclick={task.loadStatus}>重试</Button>
	{/if}

	{#if status}
		{#if status.task.current}
			<div class="space-y-1 break-all rounded-md border p-3 text-sm" role="status">
				<p>当前操作：{operations[status.task.current.operation] ?? '账号操作'}</p>
				<p>当前阶段：{stages[status.task.current.stage] ?? '处理中'}</p>
				{#if status.task.current.folder}<p>
						当前收藏夹：{status.task.current.folder}
					</p>{/if}
				{#if status.task.current.video}<p>当前视频：{status.task.current.video}</p>{/if}
			</div>
		{/if}
		<div class="grid gap-3 text-sm md:grid-cols-3">
			<div>
				<span class="text-muted-foreground">开始运行：</span>

				{status.task.last_run ? new Date(status.task.last_run).toLocaleString('zh-CN') : '—'}
			</div>

			<div>
				<span class="text-muted-foreground">运行结束：</span>

				{status.task.last_finish ? new Date(status.task.last_finish).toLocaleString('zh-CN') : '—'}
			</div>

			<div>
				<span class="text-muted-foreground">下次运行：</span>

				{status.task.next_run ? new Date(status.task.next_run).toLocaleString('zh-CN') : '—'}
			</div>
		</div>
		<section class="space-y-3 rounded-md border p-3" aria-label="本轮同步统计">
			{#if status.task.last_sync}
				<h3 class="text-sm font-medium">
					{status.task.last_sync.finished_at ? '最近一轮同步' : '本轮同步'}
				</h3>
				<p class="text-muted-foreground text-xs">
					开始于 {new Date(status.task.last_sync.started_at).toLocaleString('zh-CN')} ·
					{status.task.last_sync.finished_at
						? status.task.last_sync.error
							? '未完成，保留已完成部分'
							: '已完成'
						: '运行中'}
				</p>
				{#if windowSummary?.rebased}<p role="status" class="text-sm">
						窗口断档，已重设起点，本批未下载。
					</p>
				{:else if windowSummary?.baseline}<p role="status" class="text-sm">
						首次窗口已记录，本批不下载历史。
					</p>{/if}
				<dl class="grid grid-cols-2 gap-3 text-sm sm:grid-cols-3 lg:grid-cols-5">
					{#each counts as [key, label] (key)}
						<div>
							<dt class="text-muted-foreground">{label}</dt>
							<dd class="mt-1 font-medium">{status.task.last_sync[key] ?? '未知'}</dd>
						</div>
					{/each}
				</dl>
				{#if status.task.last_sync.error}
					<p class="text-destructive break-words text-sm" role="status">
						本轮结果：{status.task.last_sync.error === 'cancelled'
							? '任务已取消或中断，请重新同步'
							: message(status.task.last_sync.error)}
					</p>
				{:else if status.task.last_sync.finished_at && status.task.last_sync.downloaded === 0}
					<p class="text-muted-foreground text-xs">
						本轮没有新下载。首次基线跳过历史；已有、不支持及复用或恢复的文件不计入新下载。详情见日志。
					</p>
				{/if}
				<p class="text-muted-foreground text-xs">
					{summaryNote}
				</p>
			{:else}
				<p class="text-muted-foreground text-sm">暂无本次运行统计</p>
			{/if}
		</section>

		{#if cooldown}<p class="text-sm" role="status">
				冷却至 {new Date(cooldown).toLocaleString('zh-CN')}，登录、恢复和手动同步均须等待。
			</p>{/if}
		{#if status.task.last_error}
			<p class="text-destructive text-sm" role="status">
				最近失败：{message(status.task.last_error)}
			</p>
		{/if}

		{#if status.paused_reason}
			<p class="text-destructive text-sm" role="status">
				暂停原因：{message(status.paused_reason)}（连续拒绝 {status.rejections} 次）
			</p>
		{/if}

		<div class="flex flex-wrap items-center gap-3">
			<Button size="sm" variant="outline" href="/logs">查看日志</Button>
			<Button size="sm" variant="outline" href="/watch?platform={platform}">查看已保存视频</Button>
			{#if status.auth_state === 'paused'}
				<Button
					size="sm"
					onclick={() => run(true)}
					disabled={$task.busy ||
						status.task.is_running ||
						(!!cooldown && new Date(cooldown).getTime() > Date.now())}
					>{$task.busy ? '验证中…' : '验证连接并恢复'}</Button
				>

				<p class="text-muted-foreground text-xs">
					恢复前验证同一账号。登录已失效时，请先在“连接账号”重新登录。
				</p>
			{:else if status.connected}
				<Button
					size="sm"
					onclick={() => run()}
					disabled={$task.busy ||
						status.task.is_running ||
						(!!cooldown && new Date(cooldown).getTime() > Date.now())}
				>
					{$task.busy
						? '触发中…'
						: status.task.is_running
							? '任务运行中'
							: status.task.last_error
								? retryLabel
								: syncLabel}
				</Button>
			{:else}
				<Button size="sm" variant="outline" href="/platforms/{platform}?view=account"
					>连接{name}</Button
				>
			{/if}
		</div>
	{:else if !$task.error}
		<p class="text-muted-foreground text-sm">正在加载任务状态…</p>
	{/if}
</section>
