<script lang="ts">
	import { onMount } from 'svelte';
	import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import PlayIcon from '@lucide/svelte/icons/play';
	import HistoryIcon from '@lucide/svelte/icons/history';
	import CoverImage from './cover-image.svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import { formatTime, playbackUrl } from '$lib/watch';
	import api from '$lib/api';
	import type { ContinueWatchingItem } from '$lib/types';
	export let search = '';
	let items: ContinueWatchingItem[] = [];
	let loading = true;
	let error = false;
	let alive = true;
	let rail: HTMLDivElement;
	async function load() {
		loading = true;
		error = false;
		try {
			const { data } = await api.getContinueWatching();
			if (alive) items = data;
		} catch {
			if (alive) error = true;
		} finally {
			if (alive) loading = false;
		}
	}
	onMount(() => {
		void load();
		return () => {
			alive = false;
		};
	});
	function scroll(direction: number) {
		rail?.scrollBy({ left: direction * rail.clientWidth * 0.8 });
	}
</script>

{#if loading || error || items.length}
	<section aria-label="继续观看" class="continue-section">
		<div class="section-heading">
			<div class="continue-title">
				<HistoryIcon size={20} />
				<h2>继续观看</h2>
			</div>
			{#if items.length}<div class="rail-actions">
					<Button
						variant="outline"
						size="icon"
						aria-label="向前浏览继续观看"
						onclick={() => scroll(-1)}><ChevronLeftIcon size={16} /></Button
					><Button
						variant="outline"
						size="icon"
						aria-label="向后浏览继续观看"
						onclick={() => scroll(1)}><ChevronRightIcon size={16} /></Button
					>
				</div>{/if}
		</div>
		{#if error}<div class="continue-error">
				<span role="alert">观看记录暂时无法加载</span><button type="button" onclick={load}
					>重试</button
				>
			</div>
		{:else}
			<div class="continue-rail" bind:this={rail} role="group" aria-label="继续观看列表">
				{#if loading}{#each Array(3) as _, i (i)}<div
							class="continue-skeleton"
							role="status"
							aria-label="正在加载观看记录"
						></div>{/each}
				{:else}{#each items as item (item.video_id)}
						<a
							class="continue-card"
							href={playbackUrl(item.video_id, item.page_id, search)}
							title={item.video_name}
						>
							<div class="continue-art">
								<CoverImage videoId={item.video_id} title={item.video_name} /><span class="resume"
									><PlayIcon size={12} fill="currentColor" />继续观看</span
								><span class="position"
									>{formatTime(item.position_ms / 1000)} / {formatTime(item.duration)}</span
								>
								<div class="track">
									<div
										style:width={`${item.duration > 0 ? Math.min(100, item.position_ms / (item.duration * 10)) : 0}%`}
									></div>
								</div>
							</div>
							<div class="continue-caption">
								<h3>{item.video_name}</h3>
								<p>
									P{item.pid} · {item.page_name === item.video_name
										? item.upper_name
										: item.page_name}
								</p>
							</div>
						</a>
					{/each}{/if}
			</div>
		{/if}
	</section>
{/if}

<style>
	.continue-title {
		display: flex;
		align-items: center;
		gap: 9px;
	}
	.continue-title > :global(svg) {
		color: var(--watch-accent);
	}
	.rail-actions {
		display: flex;
		gap: 6px;
	}
	.continue-rail {
		display: flex;
		gap: 18px;
		overflow-x: auto;
		scroll-snap-type: x mandatory;
		scroll-behavior: smooth;
		padding: 4px 4px 10px;
		margin: -4px;
		scrollbar-width: thin;
		scrollbar-color: var(--watch-line) transparent;
		outline-offset: 4px;
	}
	.continue-card {
		flex: 0 0 248px;
		min-width: 0;
		scroll-snap-align: start;
		border-radius: 10px;
		outline-offset: 3px;
	}
	.continue-art {
		aspect-ratio: 16/9;
		border-radius: 10px;
		position: relative;
		overflow: hidden;
	}
	.resume {
		position: absolute;
		top: 9px;
		left: 9px;
		display: flex;
		align-items: center;
		gap: 4px;
		padding: 5px 7px;
		background: #0b1224b8;
		color: white;
		border-radius: 5px;
		font-size: 11px;
	}
	.position {
		position: absolute;
		right: 8px;
		bottom: 10px;
		color: white;
		font-size: 10px;
		background: #0b1224b8;
		border-radius: 4px;
		padding: 3px 5px;
		font-variant-numeric: tabular-nums;
	}
	.track {
		position: absolute;
		bottom: 0;
		height: 4px;
		width: 100%;
		background: #ffffff60;
	}
	.track > div {
		height: 100%;
		background: #3b82f6;
	}
	.continue-caption {
		padding: 9px 1px 0;
	}
	h3 {
		font-size: 13px;
		font-weight: 600;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.continue-caption p {
		font-size: 11px;
		color: var(--watch-muted);
		margin-top: 4px;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.continue-skeleton {
		flex: 0 0 248px;
		aspect-ratio: 16/9;
		border-radius: 10px;
		background: var(--watch-placeholder);
	}
	.continue-error {
		display: flex;
		align-items: center;
		gap: 14px;
		color: var(--watch-muted);
		font-size: 13px;
	}
	.continue-error button {
		color: var(--watch-accent);
	}
	@media (max-width: 767px) {
		.continue-card,
		.continue-skeleton {
			flex-basis: 218px;
		}
	}
	@media (max-width: 479px) {
		.continue-card,
		.continue-skeleton {
			flex-basis: 204px;
		}
		.continue-rail {
			gap: 12px;
		}
	}
</style>
