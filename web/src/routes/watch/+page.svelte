<script lang="ts">
	import { onMount } from 'svelte';
	import api from '$lib/api';
	import ContinueWatching from '$lib/components/continue-watching.svelte';
	import Pagination from '$lib/components/pagination.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Card, CardContent, CardHeader, CardTitle } from '$lib/components/ui/card/index.js';
	import { Input } from '$lib/components/ui/input/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	import type { ApiError, VideosResponse } from '$lib/types';

	const pageSize = 20;
	let videosData: VideosResponse | null = null;
	let currentPage = 0;
	let search = '';
	let query = '';
	let loading = true;
	let error = '';
	let requestId = 0;

	async function loadVideos(page = 0) {
		const request = ++requestId;
		currentPage = page;
		loading = true;
		error = '';
		try {
			const { data } = await api.getVideos({ page, page_size: pageSize, query, saved_only: true });
			if (request === requestId) videosData = data;
		} catch (cause) {
			if (request === requestId) error = (cause as ApiError).message;
		} finally {
			if (request === requestId) loading = false;
		}
	}

	onMount(() => {
		void loadVideos();
		return () => {
			requestId++;
		};
	});
</script>

<svelte:head>
	<title>观看中心 - favkeep</title>
</svelte:head>

<div class="space-y-8">
	<header class="space-y-2">
		<h1 class="text-2xl font-semibold">观看中心</h1>
		<p class="text-muted-foreground">浏览已保存的视频，或从上次的位置继续观看。</p>
	</header>
	<ContinueWatching />
	<section aria-label="已保存视频" class="space-y-4">
		<h2 class="text-xl font-semibold">已保存视频</h2>
		<form
			class="flex flex-wrap items-end gap-2"
			onsubmit={(event) => {
				event.preventDefault();
				query = search.trim();
				void loadVideos();
			}}
		>
			<div class="w-full space-y-2 sm:max-w-sm">
				<Label for="watch-query">搜索标题或 BV 号</Label>
				<Input id="watch-query" type="search" bind:value={search} placeholder="输入标题或 BV 号" />
			</div>
			<Button type="submit">搜索</Button>
		</form>
		{#if loading}
			<p class="text-muted-foreground" role="status">正在加载视频…</p>
		{:else if error}
			<p class="text-destructive" role="alert">无法加载视频：{error}</p>
			<Button variant="outline" onclick={() => loadVideos(currentPage)}>重试</Button>
		{:else if videosData?.videos.length}
			<p class="text-muted-foreground text-sm">共 {videosData.total_count} 个视频</p>
			<div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
				{#each videosData.videos as video (video.id)}
					<a
						href="/watch/{video.id}"
						class="min-w-0 rounded-lg focus-visible:outline-2 focus-visible:outline-offset-4"
					>
						<Card class="h-full transition-shadow hover:shadow-md">
							<CardHeader
								><CardTitle class="line-clamp-2 text-base">{video.name}</CardTitle></CardHeader
							>
							<CardContent class="space-y-2">
								<p class="text-muted-foreground truncate text-sm">{video.upper_name}</p>
								<p class="text-sm">开始观看</p>
							</CardContent>
						</Card>
					</a>
				{/each}
			</div>
			<Pagination
				{currentPage}
				totalPages={Math.ceil(videosData.total_count / pageSize)}
				onPageChange={loadVideos}
			/>
		{:else}
			<p class="text-muted-foreground">
				{query ? '没有匹配的已保存视频。' : '暂无已保存视频，下载完成后会显示在这里。'}
			</p>
		{/if}
	</section>
</div>
