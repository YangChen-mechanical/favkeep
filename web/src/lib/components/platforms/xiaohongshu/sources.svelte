<script lang="ts">
	import { onMount } from 'svelte';
	import api from '$lib/api';
	import type { ApiError, XiaohongshuSource } from '$lib/types';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Checkbox } from '$lib/components/ui/checkbox/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';
	import { toast } from 'svelte-sonner';
	let source: XiaohongshuSource | null = null;
	let error = '';
	let saving = false;
	async function load() {
		try {
			source = (await api.getXiaohongshuSource()).data;
			error = '';
		} catch (e) {
			error = (e as ApiError).message;
		}
	}
	async function toggle(enabled: boolean) {
		if (saving) return;
		saving = true;
		try {
			source = (await api.updateXiaohongshuSource(enabled)).data;
			toast.success(
				enabled
					? source.baseline_done
						? '来源已启用，沿用已有起点'
						: '来源已启用；首次同步不下载历史'
					: '来源已停用，已有文件与观看记录保留'
			);
		} catch (e) {
			toast.error('更新来源失败', { description: (e as ApiError).message });
		} finally {
			saving = false;
		}
	}
	const results: Record<string, string> = {
		baseline: '首次窗口已记录',
		complete: '窗口检查完成',
		rebased: '窗口断档，已重设起点，本批未下载'
	};
	onMount(() => {
		void load();
	});
</script>

<section class="space-y-4 rounded-lg border p-4" aria-label="小红书收藏来源">
	<h2 class="font-semibold">全部收藏</h2>
	<p class="text-muted-foreground text-sm">
		仅检查页面前 50
		个不同条目，图文占窗口位置但不下载。首次成功扫描只记录历史；之后仅保存第一个已见条目前、从未见过的连续前缀中的新发现视频。
	</p>
	<p class="text-muted-foreground text-sm">
		窗口完全无重叠时自动建立新起点，本批不下载。窗口外新增可能遗漏；历史重新排序也可能影响判断，不以作品发布时间冒充收藏时间。
	</p>
	{#if error}<p role="alert" class="text-destructive text-sm">{error}</p>
		<Button variant="outline" onclick={load}>重新加载</Button>{/if}
	{#if source}
		<div class="flex flex-wrap items-center gap-3">
			<Checkbox
				id="xhs-source"
				checked={source.enabled}
				disabled={saving}
				onCheckedChange={(v) => toggle(v === true)}
			/>
			<label for="xhs-source" class="text-sm">自动保存新发现视频</label>
			<Badge variant={source.enabled ? 'default' : 'outline'}
				>{source.enabled ? '已启用' : '已停用'}</Badge
			>
		</div>
		<dl class="grid grid-cols-1 gap-3 text-sm sm:grid-cols-2">
			<div>
				<dt class="text-muted-foreground">历史起点</dt>
				<dd>{source.baseline_done ? '已建立' : '等待首次成功扫描'}</dd>
			</div>
			<div>
				<dt class="text-muted-foreground">最近窗口条目数</dt>
				<dd>{source.item_count} / 50</dd>
			</div>
			<div>
				<dt class="text-muted-foreground">最近成功扫描</dt>
				<dd>
					{source.last_scan ? new Date(source.last_scan * 1000).toLocaleString('zh-CN') : '—'}
				</dd>
			</div>
			<div>
				<dt class="text-muted-foreground">最近结果</dt>
				<dd>
					{source.last_result ? results[source.last_result] || '需要查看任务日志' : '尚未执行'}
				</dd>
			</div>
		</dl>
	{:else if !error}<p role="status">正在加载来源…</p>{/if}
	<p class="text-muted-foreground text-xs">
		默认停用。停用或取消收藏均保留已保存文件、观看记录与已有下载资格；重新启用后继续按窗口规则检查。
	</p>
</section>
