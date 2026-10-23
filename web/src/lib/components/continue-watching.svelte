<script lang="ts">
	import { onMount } from 'svelte';
	import { Card, CardContent, CardHeader, CardTitle } from '$lib/components/ui/card/index.js';
	import { Progress } from '$lib/components/ui/progress/index.js';
	import api from '$lib/api';
	import type { ApiError, ContinueWatchingItem } from '$lib/types';

	let items: ContinueWatchingItem[] = [];
	let loading = true;
	let error = '';
	onMount(() => {
		let active = true;
		api
			.getContinueWatching()
			.then(({ data }) => {
				if (active) items = data;
			})
			.catch((cause: ApiError) => {
				if (active)
					error =
						cause.status === 401 ? '请先在设置中完成认证。' : '无法加载观看记录，请刷新重试。';
			})
			.finally(() => {
				if (active) loading = false;
			});
		return () => {
			active = false;
		};
	});
</script>

<section aria-label="继续观看" class="space-y-3">
	<h2 class="text-xl font-semibold">继续观看</h2>
	{#if loading}
		<p class="text-muted-foreground" role="status">正在加载观看记录…</p>
	{:else if error}
		<p role="alert" class="text-muted-foreground">{error}</p>
	{:else if items.length === 0}
		<p class="text-muted-foreground">还没有未看完的视频，打开已保存视频即可开始观看。</p>
	{:else}
		<div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
			{#each items as item (item.page_id)}
				<a
					href="/watch/{item.video_id}?part={item.page_id}"
					class="rounded-lg focus-visible:outline-2 focus-visible:outline-offset-4"
				>
					<Card class="h-full">
						<CardHeader><CardTitle class="text-base">{item.video_name}</CardTitle></CardHeader>
						<CardContent class="space-y-2">
							<p class="text-muted-foreground text-sm">P{item.pid} · {item.page_name}</p>
							<Progress
								value={item.duration > 0
									? Math.min(100, item.position_ms / (item.duration * 10))
									: 0}
							/>
							<p class="text-muted-foreground text-sm">
								已观看 {Math.floor(item.position_ms / 60_000)} 分钟
							</p>
						</CardContent>
					</Card>
				</a>
			{/each}
		</div>
	{/if}
</section>
