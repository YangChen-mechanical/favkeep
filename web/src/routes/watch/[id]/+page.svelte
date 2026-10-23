<script lang="ts">
	import { onDestroy } from 'svelte';
	import { page } from '$app/stores';
	import api from '$lib/api';
	import LocalPlayer from '$lib/components/local-player.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import type { ApiError, VideoResponse } from '$lib/types';

	let videoData: VideoResponse | null = null;
	let loading = true;
	let error = '';
	let requestId = 0;

	async function loadVideo(id: string) {
		const request = ++requestId;
		videoData = null;
		error = '';
		const videoId = Number(id);
		if (!Number.isSafeInteger(videoId) || videoId <= 0 || videoId > 2_147_483_647) {
			error = '无效的视频 ID';
			loading = false;
			return;
		}
		loading = true;
		try {
			const { data } = await api.getVideo(videoId);
			if (request === requestId) videoData = data;
		} catch (cause) {
			if (request === requestId) error = (cause as ApiError).message;
		} finally {
			if (request === requestId) loading = false;
		}
	}

	$: void loadVideo($page.params.id!);
	onDestroy(() => {
		requestId++;
	});
</script>

<svelte:head>
	<title>{videoData?.video.name || '播放'} - favkeep</title>
</svelte:head>

<div class="mx-auto max-w-6xl space-y-6">
	<div class="flex flex-wrap gap-2">
		<Button variant="outline" href="/watch">返回观看中心</Button>
		{#if videoData}<Button variant="ghost" href="/video/{videoData.video.id}">管理此视频</Button
			>{/if}
	</div>
	{#if loading}
		<p class="text-muted-foreground" role="status">正在加载视频…</p>
	{:else if error}
		<p class="text-destructive" role="alert">无法加载视频：{error}</p>
		<Button variant="outline" onclick={() => loadVideo($page.params.id!)}>重试</Button>
	{:else if videoData}
		<header class="space-y-2">
			<h1 class="text-2xl font-semibold">{videoData.video.name}</h1>
			<p class="text-muted-foreground">{videoData.video.upper_name}</p>
		</header>
		{#key videoData.video.id}<LocalPlayer pages={videoData.pages} />{/key}
	{/if}
</div>
