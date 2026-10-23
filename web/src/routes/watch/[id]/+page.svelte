<script lang="ts">
	import { onDestroy } from 'svelte';
	import { page } from '$app/stores';
	import { onNavigate } from '$app/navigation';
	import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
	import SettingsIcon from '@lucide/svelte/icons/settings-2';
	import { toast } from 'svelte-sonner';
	import api from '$lib/api';
	import LocalPlayer from '$lib/components/local-player.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import { returnUrl } from '$lib/watch';
	import type { ApiError, VideoResponse } from '$lib/types';

	let videoData: VideoResponse | null = null;
	let loading = true;
	let error = '';
	let requestId = 0;
	let loadedId = '';
	let player: LocalPlayer;
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
	$: if ($page.params.id && loadedId !== $page.params.id) {
		loadedId = $page.params.id;
		void loadVideo(loadedId);
	}
	onNavigate(async () => {
		if (player && !(await player.flush()))
			toast.error('本次观看进度未保存，请留意网络或其他窗口的记录。');
	});
	onDestroy(() => {
		requestId++;
	});
</script>

<svelte:head><title>{videoData?.video.name || '播放'} - favkeep</title></svelte:head>
<div class="playback-page">
	<div class="playback-breadcrumb">
		<a href={returnUrl($page.url.searchParams.get('return'))}
			><ChevronLeftIcon size={16} />返回媒体库</a
		>{#if videoData}<a class="manage-video" href="/video/{videoData.video.id}"
				><SettingsIcon size={14} />管理此视频</a
			>{/if}
	</div>
	{#if loading}<div class="player-skeleton" role="status" aria-label="正在加载视频"></div>
	{:else if error}<div class="watch-empty">
			<p role="alert">无法加载视频：{error}</p>
			<Button variant="outline" onclick={() => loadVideo($page.params.id!)}>重试</Button>
		</div>
	{:else if videoData}{#key videoData.video.id}<LocalPlayer
				bind:this={player}
				video={videoData.video}
				pages={videoData.pages}
				playableIds={videoData.playable_page_ids ??
					videoData.pages.filter((p) => p.download_status[1] === 7).map((p) => p.id)}
				progress={videoData.watch_progress ?? []}
				initialPageId={videoData.watch_summary?.resume_page_id}
			/>{/key}{/if}
</div>

<style>
	.playback-breadcrumb {
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: 12px;
		margin-bottom: 22px;
	}
	.playback-breadcrumb a {
		display: flex;
		align-items: center;
		gap: 5px;
		color: var(--watch-muted);
		font-size: 13px;
		border-radius: 4px;
	}
	.playback-breadcrumb a:hover {
		color: var(--watch-accent);
	}
	.manage-video {
		font-size: 12px !important;
	}
	.player-skeleton {
		aspect-ratio: 16/9;
		max-height: 65vh;
		background: var(--watch-placeholder);
		border-radius: 12px;
	}
</style>
