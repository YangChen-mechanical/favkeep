<script lang="ts">
	import { platformLabels, sourceVideoUrl } from '$lib/platform';
	import { onMount } from 'svelte';
	import CheckIcon from '@lucide/svelte/icons/check';
	import CircleIcon from '@lucide/svelte/icons/circle';
	import { Button } from '$lib/components/ui/button/index.js';
	import { formatTime } from '$lib/watch';
	import api from '$lib/api';
	import type {
		ApiError,
		PageInfo,
		VideoInfo,
		WatchProgress,
		WatchProgressUpdate
	} from '$lib/types';

	export let page: PageInfo;
	export let video: VideoInfo;
	export let onProgress: (record: WatchProgress) => void;
	let player: HTMLVideoElement;
	let loaded = false;
	let restored = false;
	let alive = true;
	let revision = 0;
	let resumeMs = 0;
	let completed = false;
	let error = '';
	let playbackError = '';
	let conflict = false;
	let pending: Omit<WatchProgressUpdate, 'revision'> | null = null;
	let saving: Promise<boolean> | null = null;
	let lastSaved = '';
	function key(position_ms: number, completed: boolean) {
		return `${position_ms}:${completed}`;
	}
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
			lastSaved = key(resumeMs, completed);
			if (data) onProgress(data);
			loaded = true;
			restored = false;
			restore();
		} catch (cause) {
			if (alive) error = `无法读取观看记录：${(cause as ApiError).message}`;
		}
	}
	export async function flush(): Promise<boolean> {
		if (conflict) return false;
		if (!loaded || !restored || !player || !Number.isFinite(player.currentTime)) return !error;
		const update = {
			position_ms: Math.min(
				page.duration * 1000,
				Math.max(0, Math.round(player.currentTime * 1000))
			),
			completed
		};
		if (!saving && !pending && key(update.position_ms, update.completed) === lastSaved) return true;
		pending = update;
		if (saving) return saving;
		saving = (async () => {
			try {
				while (pending && !conflict) {
					const update = pending;
					pending = null;
					const { data } = await api.saveWatchProgress(page.id, { ...update, revision }, true);
					revision = data.revision;
					lastSaved = key(update.position_ms, update.completed);
					if (alive) {
						error = '';
						onProgress(data);
					}
				}
				return true;
			} catch (cause) {
				if ((cause as ApiError).status === 409) {
					conflict = true;
					if (alive) {
						player.pause();
						error = '另一窗口更新了观看记录，请重新加载后继续。';
					}
				} else if (alive) error = '观看记录保存失败，请重试后再离开。';
				return false;
			} finally {
				saving = null;
			}
		})();
		return saving;
	}
	onMount(() => {
		void loadProgress();
		const save = () => {
			void flush();
		};
		const timer = setInterval(() => {
			if (player && !player.paused) save();
		}, 15000);
		window.addEventListener('pagehide', save);
		return () => {
			save();
			alive = false;
			clearInterval(timer);
			window.removeEventListener('pagehide', save);
		};
	});
</script>

<div class="player-main">
	<div class="video-stage">
		<!-- shortcut: ASS subtitles need WebVTT export before browser text tracks can be added. -->
		<!-- svelte-ignore a11y_media_has_caption -->
		<video
			bind:this={player}
			src="/api/media/pages/{page.id}"
			poster="/api/media/videos/{video.id}/cover"
			controls
			playsinline
			preload="metadata"
			aria-label="P{page.pid}：{page.name}"
			onloadedmetadata={restore}
			onpause={() => {
				void flush();
			}}
			onseeked={() => {
				void flush();
			}}
			onended={() => {
				completed = true;
				void flush();
			}}
			onerror={() => {
				playbackError = '无法播放本地文件，请检查文件是否存在及浏览器是否支持其编码。';
			}}
		></video>
	</div>
	<div class="video-details">
		<div class="video-kicker">
			<span class="saved-dot"></span>已保存到本地<span>·</span>{platformLabels[video.platform]}<span
				>·</span
			>{formatTime(page.duration)}<span>·</span>P{page.pid}
		</div>
		<h1>{video.name}</h1>
		<div class="details-bottom">
			<div class="author">
				<span class="avatar">{video.upper_name.slice(0, 1) || 'UP'}</span><span
					>{video.upper_name}</span
				>
			</div>
			{#if sourceVideoUrl(video)}<Button
					variant="outline"
					size="sm"
					href={sourceVideoUrl(video)!}
					target="_blank"
					rel="noopener noreferrer">在{platformLabels[video.platform]}打开</Button
				>{/if}
			<Button
				variant="outline"
				size="sm"
				disabled={!loaded || conflict}
				onclick={() => {
					completed = !completed;
					void flush();
				}}
				>{#if completed}<CheckIcon size={14} />已看 · 标记未看{:else}<CircleIcon
						size={14}
					/>标记已看{/if}</Button
			>
		</div>
		{#if error}<div class="player-error">
				<p role="alert">{error}</p>
				<Button
					size="sm"
					variant="outline"
					onclick={() => {
						if (loaded && !conflict) void flush();
						else void loadProgress();
					}}>{conflict || !loaded ? '重新加载观看记录' : '重试保存'}</Button
				>
			</div>{/if}
		{#if playbackError}<p role="alert" class="player-error">{playbackError}</p>{/if}
	</div>
</div>

<style>
	.player-main {
		min-width: 0;
	}
	.video-stage {
		aspect-ratio: 16/9;
		max-height: 70vh;
		background: #070a12;
		border-radius: 12px;
		overflow: hidden;
	}
	video {
		width: 100%;
		height: 100%;
		object-fit: contain;
		display: block;
	}
	.video-details {
		padding: 22px 0;
	}
	.video-kicker {
		display: flex;
		align-items: center;
		gap: 9px;
		color: var(--watch-muted);
		font-size: 12px;
	}
	.saved-dot {
		width: 6px;
		height: 6px;
		border-radius: 50%;
		background: #16a34a;
	}
	h1 {
		font-size: 21px;
		font-weight: 650;
		line-height: 1.55;
		margin: 10px 0 18px;
		letter-spacing: -0.4px;
	}
	.details-bottom {
		display: flex;
		align-items: center;
		justify-content: space-between;
		flex-wrap: wrap;
		gap: 14px;
	}
	.author {
		display: flex;
		align-items: center;
		gap: 9px;
		color: var(--watch-muted);
		font-size: 13px;
	}
	.avatar {
		display: grid;
		place-items: center;
		width: 32px;
		height: 32px;
		background: var(--accent);
		color: var(--watch-accent);
		border-radius: 50%;
		font-weight: 600;
	}
	.player-error {
		margin-top: 16px;
		color: #dc2626;
		font-size: 13px;
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: 10px;
	}
	@media (max-width: 479px) {
		h1 {
			font-size: 17px;
		}
		.video-details {
			padding: 18px 0;
		}
		.video-stage {
			border-radius: 9px;
		}
	}
</style>
