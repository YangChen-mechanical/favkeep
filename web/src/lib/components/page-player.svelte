<script lang="ts">
	import { onMount } from 'svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import api from '$lib/api';
	import type { ApiError, PageInfo } from '$lib/types';

	export let page: PageInfo;
	let player: HTMLVideoElement;
	let loaded = false;
	let restored = false;
	let alive = true;
	let saving = false;
	let revision = 0;
	let resumeMs = 0;
	let completed = false;
	let error = '';
	let playbackError = '';
	let conflict = false;
	let pending: { position_ms: number; completed: boolean } | null = null;

	function restore() {
		if (!restored && loaded && player?.readyState >= 1) {
			player.currentTime = Math.min(resumeMs / 1000, Math.max(0, player.duration - 1));
			restored = true;
		}
	}

	async function loadProgress() {
		loaded = false;
		error = '';
		conflict = false;
		try {
			const { data } = await api.getWatchProgress(page.id);
			if (!alive) return;
			revision = data?.revision ?? 0;
			resumeMs = data?.position_ms ?? 0;
			completed = data?.completed ?? false;
			loaded = true;
			restored = false;
			restore();
		} catch (cause) {
			if (alive) error = `无法读取观看记录：${(cause as ApiError).message}`;
		}
	}

	async function save() {
		if (!loaded || !restored || conflict || !player || !Number.isFinite(player.currentTime)) return;
		pending = {
			position_ms: Math.min(
				page.duration * 1000,
				Math.max(0, Math.round(player.currentTime * 1000))
			),
			completed
		};
		if (saving) return;
		saving = true;
		try {
			while (pending && alive && !conflict) {
				const update = pending;
				pending = null;
				const { data } = await api.saveWatchProgress(page.id, { ...update, revision });
				revision = data.revision;
				if (alive) error = '';
			}
		} catch (cause) {
			if (!alive) return;
			if ((cause as ApiError).status === 409) {
				conflict = true;
				player.pause();
				error = '另一窗口更新了观看记录，请重新加载后继续。';
			} else {
				error = '观看记录保存失败，网络恢复后将重试。';
			}
		} finally {
			saving = false;
		}
	}

	function ended() {
		completed = true;
		void save();
	}

	onMount(() => {
		void loadProgress();
		const timer = setInterval(() => {
			if (player && !player.paused) void save();
		}, 15_000);
		return () => {
			alive = false;
			clearInterval(timer);
		};
	});
</script>

<div class="space-y-3">
	<!-- shortcut: upstream ASS subtitles are not WebVTT; add text tracks when browser subtitle export is implemented. -->
	<!-- svelte-ignore a11y_media_has_caption -->
	<video
		bind:this={player}
		src="/api/media/pages/{page.id}"
		class="bg-muted max-h-[70vh] w-full rounded-lg"
		controls
		playsinline
		preload="metadata"
		aria-label="P{page.pid}：{page.name}"
		onloadedmetadata={restore}
		onpause={() => {
			void save();
		}}
		onseeked={() => {
			void save();
		}}
		onended={ended}
		onerror={() => {
			playbackError = '无法播放本地文件，请检查文件是否存在及浏览器是否支持其编码。';
		}}
	></video>
	<div class="flex flex-wrap items-center gap-3">
		<span class="text-muted-foreground text-sm">{completed ? '已看' : '未看完'}</span>
		<Button
			variant="outline"
			size="sm"
			disabled={!loaded || conflict}
			onclick={() => {
				completed = !completed;
				void save();
			}}
		>
			{completed ? '标记未看' : '标记已看'}
		</Button>
		{#if error}
			<Button
				variant="outline"
				size="sm"
				onclick={() => {
					if (loaded && !conflict) void save();
					else void loadProgress();
				}}
			>
				{conflict || !loaded ? '重新加载观看记录' : '重试保存'}
			</Button>
		{/if}
	</div>
	{#if error}<p role="alert" class="text-destructive text-sm">{error}</p>{/if}
	{#if playbackError}<p role="alert" class="text-destructive text-sm">{playbackError}</p>{/if}
</div>
