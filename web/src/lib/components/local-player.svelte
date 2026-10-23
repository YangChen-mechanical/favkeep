<script lang="ts">
	import { SvelteMap } from 'svelte/reactivity';
	import { page as routePage } from '$app/stores';
	import { replaceState } from '$app/navigation';
	import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import ListVideoIcon from '@lucide/svelte/icons/list-video';
	import PlayIcon from '@lucide/svelte/icons/play';
	import CheckIcon from '@lucide/svelte/icons/check';
	import { Button } from '$lib/components/ui/button/index.js';
	import { formatTime } from '$lib/watch';
	import PagePlayer from './page-player.svelte';
	import type { PageInfo, VideoInfo, WatchProgress } from '$lib/types';

	export let video: VideoInfo;
	export let pages: PageInfo[];
	export let playableIds: number[];
	export let progress: WatchProgress[];
	export let initialPageId: number | undefined;
	const records = new SvelteMap(progress.map((w) => [w.page_id, w]));
	let selectedId: number | null = null;
	let lastRequested: number | undefined;
	let switching = false;
	let player: PagePlayer;
	$: downloaded = pages.filter((p) => playableIds.includes(p.id));
	$: requested = Number($routePage.url.searchParams.get('part'));
	$: if (downloaded.length && !downloaded.some((p) => p.id === selectedId))
		selectedId = downloaded.some((p) => p.id === requested)
			? requested
			: downloaded.some((p) => p.id === initialPageId)
				? initialPageId!
				: downloaded[0].id;
	$: if (requested !== lastRequested) {
		lastRequested = requested;
		if (downloaded.some((p) => p.id === requested)) selectedId = requested;
	}
	$: selected = downloaded.find((p) => p.id === selectedId);
	$: index = downloaded.findIndex((p) => p.id === selectedId);
	export async function flush(): Promise<boolean> {
		return player ? player.flush() : true;
	}
	async function choose(id: number) {
		if (switching || id === selectedId) return;
		switching = true;
		try {
			if (!(await flush())) return;
			selectedId = id;
			const url = new URL($routePage.url);
			url.searchParams.set('part', String(id));
			replaceState(url, $routePage.state);
		} finally {
			switching = false;
		}
	}
</script>

<div class="playback-grid">
	{#if selected}{#key selected.id}<PagePlayer
				bind:this={player}
				page={selected}
				{video}
				onProgress={(record) => {
					records.set(record.page_id, record);
				}}
			/>{/key}
	{:else}<div class="watch-empty">
			<h2>暂时没有可播放的分 P</h2>
			<p>等待视频保存完成，或在管理页检查下载状态。</p>
			<Button href="/video/{video.id}" variant="outline">查看下载状态</Button>
		</div>{/if}
	<aside class="parts-panel" aria-label="选集">
		<div class="parts-heading">
			<div>
				<ListVideoIcon size={19} />
				<h2>选集</h2>
			</div>
			<span>{downloaded.length} / {pages.length} 已保存</span>
		</div>
		<div class="parts-navigation">
			<Button
				variant="outline"
				size="sm"
				disabled={index <= 0 || switching}
				onclick={() => choose(downloaded[index - 1].id)}><ChevronLeftIcon size={15} />上一 P</Button
			><Button
				variant="outline"
				size="sm"
				disabled={index < 0 || index >= downloaded.length - 1 || switching}
				onclick={() => choose(downloaded[index + 1].id)}
				>下一 P<ChevronRightIcon size={15} /></Button
			>
		</div>
		<div class="parts-list">
			{#each pages as part (part.id)}
				{@const saved = playableIds.includes(part.id)}
				{@const record = records.get(part.id)}
				<button
					type="button"
					class="part"
					class:selected={part.id === selectedId}
					disabled={!saved || switching}
					aria-current={part.id === selectedId ? 'true' : undefined}
					onclick={() => choose(part.id)}
					title={part.name}
				>
					<span class="part-index"
						>{#if part.id === selectedId}<PlayIcon
								size={13}
								fill="currentColor"
							/>{:else if record?.completed}<CheckIcon size={14} />{:else}{String(
								part.pid
							).padStart(2, '0')}{/if}</span
					>
					<span class="part-content"
						><span class="part-title">{part.name}</span><span class="part-meta"
							>{formatTime(part.duration)}<span>·</span>{!saved
								? '未保存'
								: record?.completed
									? '已看'
									: record?.position_ms
										? `看到 ${formatTime(record.position_ms / 1000)}`
										: '未看'}</span
						></span
					>
				</button>
			{/each}
		</div>
	</aside>
</div>

<style>
	.playback-grid {
		display: grid;
		grid-template-columns: minmax(0, 1fr);
		gap: 24px;
		align-items: start;
	}
	.parts-panel {
		min-width: 0;
		background: var(--watch-surface);
		border: 1px solid var(--watch-line);
		border-radius: 12px;
		overflow: hidden;
	}
	.parts-heading {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 18px 16px 14px;
		gap: 8px;
	}
	.parts-heading > div {
		display: flex;
		align-items: center;
		gap: 8px;
	}
	.parts-heading h2 {
		font-size: 15px;
		font-weight: 650;
	}
	.parts-heading > span {
		font-size: 11px;
		color: var(--watch-muted);
		white-space: nowrap;
	}
	.parts-navigation {
		display: flex;
		justify-content: space-between;
		gap: 8px;
		padding: 0 16px 16px;
		border-bottom: 1px solid var(--watch-line);
	}
	.parts-list {
		max-height: 480px;
		overflow-y: auto;
		padding: 8px;
		scrollbar-width: thin;
	}
	.part {
		width: 100%;
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 12px 9px;
		text-align: left;
		border-radius: 8px;
	}
	.part:hover:not(:disabled) {
		background: var(--watch-bg);
	}
	.part.selected {
		background: var(--accent);
		color: var(--watch-accent);
	}
	.part:disabled:not(.selected) {
		opacity: 0.45;
	}
	.part-index {
		flex-shrink: 0;
		width: 22px;
		display: flex;
		justify-content: center;
		font-size: 12px;
		font-variant-numeric: tabular-nums;
		color: var(--watch-muted);
	}
	.selected .part-index {
		color: var(--watch-accent);
	}
	.part-content {
		min-width: 0;
	}
	.part-title {
		font-size: 12px;
		font-weight: 550;
		line-height: 1.6;
		display: -webkit-box;
		-webkit-line-clamp: 2;
		line-clamp: 2;
		-webkit-box-orient: vertical;
		overflow: hidden;
	}
	.part-meta {
		display: flex;
		gap: 7px;
		align-items: center;
		font-size: 10px;
		color: var(--watch-muted);
		margin-top: 5px;
	}
	@media (min-width: 1024px) {
		.playback-grid {
			grid-template-columns: minmax(0, 1fr) 292px;
		}
	}
</style>
