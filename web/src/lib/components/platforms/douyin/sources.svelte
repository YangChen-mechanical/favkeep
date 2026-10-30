<script lang="ts">
	import { douyinMessage } from '$lib/platform';
	import { onMount } from 'svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Checkbox } from '$lib/components/ui/checkbox/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';
	import * as Table from '$lib/components/ui/table/index.js';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import { toast } from 'svelte-sonner';
	import type { ApiError, DouyinFolder } from '$lib/types';
	import api from '$lib/api';
	let douyinFolders: DouyinFolder[] = [];
	let douyinLoading = false;
	let douyinError = '';
	let douyinSaving = '';
	async function loadDouyinFolders(refresh = false) {
		douyinLoading = true;
		douyinError = '';
		try {
			const response = refresh ? await api.refreshDouyinFolders() : await api.getDouyinFolders();
			douyinFolders = response.data.folders;
		} catch (error) {
			douyinError = (error as ApiError).message;
		} finally {
			douyinLoading = false;
		}
	}
	async function toggleDouyinFolder(folder: DouyinFolder, enabled: boolean) {
		if (douyinSaving) return;
		douyinSaving = folder.id;
		try {
			const response = await api.updateDouyinFolder(folder.id, enabled);
			douyinFolders = douyinFolders.map((item) => (item.id === folder.id ? response.data : item));
			toast.success(
				enabled
					? folder.baseline_at
						? '已启用，沿用已有基线，恢复新增视频同步'
						: '已启用，首次同步将建立基线，不下载历史内容'
					: '已停止同步，本地文件和观看记录保留'
			);
		} catch (error) {
			toast.error('更新收藏夹失败', { description: (error as ApiError).message });
		} finally {
			douyinSaving = '';
		}
	}
	// 初始化
	onMount(() => {
		void loadDouyinFolders();
	});
</script>

<section class="space-y-4" aria-label="抖音收藏夹">
	<div class="flex flex-wrap items-center justify-between gap-3">
		<div>
			<h2 class="font-semibold">抖音收藏夹</h2>
			<p class="text-muted-foreground mt-1 text-sm">
				仅同步勾选的收藏夹。取消勾选保留本地文件和观看记录，重新勾选沿用原有历史基线，恢复新增同步。
			</p>
		</div>

		<Button
			variant="outline"
			size="sm"
			onclick={() => loadDouyinFolders(true)}
			disabled={douyinLoading || !!douyinSaving}
		>
			<RefreshCwIcon class="mr-2 h-4 w-4" />
			{douyinLoading ? '加载中…' : '刷新抖音收藏夹'}
		</Button>
	</div>

	<p class="text-muted-foreground text-xs">
		首次同步完整扫描并建立基线，不下载当时已有的历史内容；后续同步新增视频。扫描成功不代表有新下载，下载结果见任务与日志。图文和实况暂不支持。列表刷新需先在

		<a href="/platforms/douyin?view=account" class="underline">连接账号</a>

		连接抖音。
	</p>

	{#if douyinError}
		<p class="text-destructive text-sm" role="alert">{douyinError}</p>

		<Button variant="outline" size="sm" onclick={() => loadDouyinFolders()}>重新加载本地列表</Button
		>
	{/if}

	{#if douyinFolders.length}
		<div class="overflow-x-auto">
			<Table.Root>
				<Table.Header>
					<Table.Row>
						<Table.Head>同步</Table.Head>
						<Table.Head>收藏夹</Table.Head>
						<Table.Head>最近扫描数量</Table.Head>
						<Table.Head>基线</Table.Head>
						<Table.Head>最近同步结果</Table.Head>
					</Table.Row>
				</Table.Header>

				<Table.Body>
					{#each douyinFolders as folder (folder.id)}
						<Table.Row>
							<Table.Cell>
								<Checkbox
									checked={folder.enabled}
									disabled={!!douyinSaving ||
										douyinLoading ||
										(folder.available === false && !folder.enabled)}
									aria-label={`同步收藏夹 ${folder.name}`}
									onCheckedChange={(enabled) => toggleDouyinFolder(folder, enabled)}
								/>
							</Table.Cell>

							<Table.Cell class="font-medium">{folder.name}</Table.Cell>
							<Table.Cell>{folder.video_count}</Table.Cell>

							<Table.Cell>
								{#if folder.baseline_at}
									<span class="text-sm">{new Date(folder.baseline_at).toLocaleString('zh-CN')}</span
									>
								{:else}
									<Badge variant="secondary">尚未建立</Badge>
								{/if}
							</Table.Cell>

							<Table.Cell>
								{#if folder.available === false}
									<span class="text-destructive text-sm">收藏夹已不可用，已停止同步</span>
								{:else if folder.last_error}
									<p class="text-destructive text-sm">{douyinMessage(folder.last_error)}</p>
								{:else if folder.last_sync_at}
									<span class="text-sm"
										>{new Date(folder.last_sync_at).toLocaleString('zh-CN')} · 同步成功</span
									>
								{:else}
									<span class="text-muted-foreground text-sm"
										>{folder.enabled ? '等待首次同步' : '未启用'}</span
									>
								{/if}
							</Table.Cell>
						</Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		</div>
	{:else if !douyinLoading && !douyinError}
		<p class="text-muted-foreground py-10 text-center text-sm">
			尚无抖音收藏夹，连接账号后点击刷新。
		</p>
	{/if}
</section>
