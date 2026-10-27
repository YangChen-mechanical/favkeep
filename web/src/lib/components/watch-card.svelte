<script lang="ts">
	import PlayIcon from '@lucide/svelte/icons/play';
	import CheckIcon from '@lucide/svelte/icons/check';
	import CoverImage from './cover-image.svelte';
	import { formatTime, playbackUrl } from '$lib/watch';
	import { platformLabels } from '$lib/platform';
	import type { VideoInfo, WatchSummary } from '$lib/types';
	export let video: VideoInfo;
	export let summary: WatchSummary | undefined;
	export let search = '';
</script>

<a
	class="watch-card"
	href={playbackUrl(video.id, summary?.resume_page_id, search)}
	title={video.name}
>
	<div class="artwork">
		<CoverImage videoId={video.id} title={video.name} />
		<span class="play"><PlayIcon size={22} fill="currentColor" /></span>
		{#if summary}<span class="duration"
				>{summary.page_count > 1 ? `${summary.page_count}P · ` : ''}{formatTime(
					summary.duration
				)}</span
			>{/if}
		{#if summary?.watch_state === 'watched'}<span class="watched"><CheckIcon size={13} />已看</span
			>{/if}
	</div>
	<div class="caption">
		<h3>{video.name}</h3>
		<div class="meta">
			<span>{video.upper_name}</span><span class="platform">{platformLabels[video.platform]}</span
			>{#if summary?.watch_state === 'in_progress'}<span class="in-progress">进行中</span
				>{:else if summary?.watch_state === 'unwatched'}<span class="unwatched">未看</span>{/if}
		</div>
	</div>
</a>

<style>
	.watch-card {
		min-width: 0;
		display: block;
		color: inherit;
		border-radius: 12px;
		outline-offset: 5px;
	}
	.watch-card:focus-visible {
		outline: 3px solid var(--watch-accent);
	}
	.artwork {
		aspect-ratio: 16/9;
		position: relative;
		border-radius: 10px;
		overflow: hidden;
		background: var(--watch-placeholder);
	}
	.duration,
	.watched {
		position: absolute;
		border-radius: 4px;
		background: #0b1224d9;
		color: white;
		font-size: 11px;
		line-height: 1;
		padding: 5px 6px;
	}
	.duration {
		right: 8px;
		bottom: 8px;
		font-variant-numeric: tabular-nums;
	}
	.watched {
		top: 8px;
		left: 8px;
		display: flex;
		align-items: center;
		gap: 3px;
	}
	.play {
		position: absolute;
		inset: 0;
		display: grid;
		place-items: center;
		color: white;
		background: #11182725;
		opacity: 0;
		transition: opacity 0.15s;
	}
	a:hover .play,
	a:focus-visible .play {
		opacity: 1;
	}
	.caption {
		padding: 11px 1px 3px;
	}
	h3 {
		font-size: 14px;
		line-height: 1.55;
		font-weight: 600;
		display: -webkit-box;
		-webkit-line-clamp: 2;
		line-clamp: 2;
		-webkit-box-orient: vertical;
		overflow: hidden;
		min-height: 2.9em;
	}
	.meta {
		display: flex;
		align-items: center;
		gap: 8px;
		margin-top: 6px;
		font-size: 12px;
		color: var(--watch-muted);
	}
	.meta > span:first-child {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.in-progress {
		color: var(--watch-accent);
		white-space: nowrap;
		font-size: 11px;
	}
	.platform {
		white-space: nowrap;
		font-size: 11px;
	}
	.unwatched {
		white-space: nowrap;
		font-size: 11px;
	}
	@media (prefers-reduced-motion: reduce) {
		.play {
			transition: none;
		}
	}
</style>
