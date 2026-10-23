<script lang="ts">
	import { SvelteURLSearchParams } from 'svelte/reactivity';
	import { onDestroy, tick } from 'svelte';
	import { page } from '$app/stores';
	import { beforeNavigate, goto } from '$app/navigation';
	import SlidersIcon from '@lucide/svelte/icons/sliders-horizontal';
	import LibraryIcon from '@lucide/svelte/icons/library';
	import SearchIcon from '@lucide/svelte/icons/search';
	import { Button } from '$lib/components/ui/button/index.js';
	import ContinueWatching from '$lib/components/continue-watching.svelte';
	import WatchCard from '$lib/components/watch-card.svelte';
	import Pagination from '$lib/components/pagination.svelte';
	import api from '$lib/api';
	import type { ApiError, VideosResponse, WatchState } from '$lib/types';

	const filters: { value: WatchState | ''; label: string }[] = [
		{ value: '', label: '全部' },
		{ value: 'unwatched', label: '未看' },
		{ value: 'in_progress', label: '进行中' },
		{ value: 'watched', label: '已看' }
	];
	let videosData: VideosResponse | null = null;
	let loading = true;
	let error = '';
	let requestId = 0;
	let lastSearch: string | null = null;
	let restoring = true;
	let state: WatchState | '';
	let sort: 'added' | 'title';
	$: query = $page.url.searchParams.get('q') || '';
	$: state = filters.find((f) => f.value === $page.url.searchParams.get('state'))?.value || '';
	$: sort = $page.url.searchParams.get('sort') === 'title' ? 'title' : 'added';
	$: currentPage = /^\d+$/.test($page.url.searchParams.get('page') || '')
		? Math.min(1_000_000, Number($page.url.searchParams.get('page')))
		: 0;
	$: summaries = new Map(
		videosData?.watch_summaries?.map((summary) => [summary.video_id, summary])
	);
	$: if ($page.url.pathname === '/watch' && lastSearch !== $page.url.search) {
		lastSearch = $page.url.search;
		void loadVideos();
	}
	async function loadVideos() {
		const request = ++requestId;
		const search = $page.url.search;
		loading = true;
		error = '';
		try {
			const { data } = await api.getVideos({
				saved_only: true,
				page: currentPage,
				page_size: 20,
				query,
				watch_state: state || undefined,
				sort
			});
			if (request !== requestId) return;
			videosData = data;
		} catch (cause) {
			if (request === requestId) error = (cause as ApiError).message;
		} finally {
			if (request === requestId) {
				loading = false;
				if (restoring) {
					restoring = false;
					await tick();
					const y = Number(sessionStorage.getItem(`watch-scroll:${search}`) || 0);
					if (y > 0) window.scrollTo({ top: y, behavior: 'instant' });
				}
			}
		}
	}
	function update(key: string, value: string) {
		const params = new SvelteURLSearchParams($page.url.search);
		params.delete('page');
		if (value) params.set(key, value);
		else params.delete(key);
		void goto(`/watch${params.size ? `?${params}` : ''}`);
	}
	beforeNavigate(() => {
		sessionStorage.setItem(`watch-scroll:${$page.url.search}`, String(window.scrollY));
	});
	onDestroy(() => {
		requestId++;
	});
</script>

<svelte:head><title>我的媒体库 - favkeep</title></svelte:head>
<div class="library-page">
	<ContinueWatching search={$page.url.search} />
	<section aria-label="已保存视频" class="library-section">
		<div class="section-heading">
			<div>
				<h1>我的媒体库<span class="library-count">{videosData?.total_count ?? '—'}</span></h1>
				<p>收藏已保存，随时慢慢看。</p>
			</div>
			<LibraryIcon class="library-mark" size={22} />
		</div>
		<div class="library-toolbar">
			<div class="watch-filters" aria-label="观看状态筛选">
				{#each filters as filter (filter.value)}<button
						type="button"
						class:active={state === filter.value}
						aria-pressed={state === filter.value}
						onclick={() => update('state', filter.value)}>{filter.label}</button
					>{/each}
			</div>
			<div class="sort-control">
				<SlidersIcon size={14} /><label class="sr-only" for="watch-sort">排序</label><select
					id="watch-sort"
					value={sort}
					onchange={(event) => update('sort', event.currentTarget.value)}
					><option value="added">最近入库</option><option value="title">标题顺序</option></select
				>
			</div>
		</div>
		{#if query}<div class="query-label">
				搜索“{query}”<button type="button" onclick={() => update('q', '')}>清除搜索</button>
			</div>{/if}
		{#if loading}
			<div class="video-grid" role="status" aria-label="正在加载视频">
				{#each Array(8) as _, i (i)}<div class="skeleton-card">
						<div class="skeleton-art"></div>
						<div class="skeleton-line"></div>
						<div class="skeleton-line short"></div>
					</div>{/each}
			</div>
		{:else if error}<div class="watch-empty">
				<p role="alert">无法加载视频：{error}</p>
				<Button onclick={loadVideos} variant="outline">重试</Button>
			</div>
		{:else if videosData?.videos.length}
			<div class="video-grid">
				{#each videosData.videos as video (video.id)}<WatchCard
						{video}
						summary={summaries.get(video.id)}
						search={$page.url.search}
					/>{/each}
			</div>
			<Pagination
				{currentPage}
				totalPages={Math.ceil(videosData.total_count / 20)}
				onPageChange={(p) => update('page', String(p))}
			/>
		{:else}<div class="watch-empty">
				<div class="empty-icon"><SearchIcon size={28} /></div>
				<h2>{query || state ? '没有找到匹配的视频' : '媒体库还空着'}</h2>
				<p>
					{query || state
						? '试试其他关键词，或清除观看状态筛选。'
						: '视频保存完成后，会带着封面出现在这里。'}
				</p>
				{#if query || state}<Button variant="outline" href="/watch">清除筛选</Button>{:else}<Button
						variant="outline"
						href="/video-sources">管理收藏来源</Button
					>{/if}
			</div>{/if}
	</section>
</div>

<style>
	.library-page {
		display: flex;
		flex-direction: column;
		gap: 32px;
	}
	.library-count {
		margin-left: 10px;
		padding: 3px 8px;
		border-radius: 6px;
		color: var(--watch-muted);
		background: var(--watch-placeholder);
		font-size: 12px;
		font-weight: 500;
		vertical-align: middle;
	}
	:global(.library-mark) {
		color: var(--watch-muted);
	}
	.library-toolbar {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		padding-bottom: 18px;
		margin-bottom: 22px;
		border-bottom: 1px solid var(--watch-line);
	}
	.watch-filters {
		display: flex;
		gap: 4px;
	}
	.watch-filters button {
		font-size: 13px;
		color: var(--watch-muted);
		border-radius: 8px;
		padding: 8px 14px;
		white-space: nowrap;
	}
	.watch-filters button.active {
		background: var(--accent);
		color: var(--watch-accent);
		font-weight: 650;
	}
	.sort-control {
		display: flex;
		align-items: center;
		gap: 7px;
		color: var(--watch-muted);
		font-size: 12px;
	}
	select {
		background: transparent;
		padding: 8px 2px;
		outline-offset: 2px;
		color: var(--watch-text);
	}
	select option {
		background: var(--watch-surface);
	}
	.video-grid {
		display: grid;
		grid-template-columns: 1fr;
		column-gap: 22px;
		row-gap: 28px;
	}
	.query-label {
		margin: -6px 0 18px;
		font-size: 13px;
		color: var(--watch-muted);
		display: flex;
		gap: 16px;
	}
	.query-label button {
		color: var(--watch-accent);
	}
	.skeleton-art {
		aspect-ratio: 16/9;
		background: var(--watch-placeholder);
		border-radius: 10px;
	}
	.skeleton-line {
		background: var(--watch-placeholder);
		height: 12px;
		margin-top: 13px;
		border-radius: 4px;
		width: 90%;
	}
	.skeleton-line.short {
		width: 50%;
		height: 10px;
	}
	.skeleton-card {
		animation: pulse 1.5s ease-in-out infinite;
	}
	@keyframes pulse {
		50% {
			opacity: 0.5;
		}
	}
	@media (min-width: 360px) {
		.video-grid {
			grid-template-columns: repeat(2, minmax(0, 1fr));
		}
	}
	@media (min-width: 768px) {
		.video-grid {
			grid-template-columns: repeat(3, minmax(0, 1fr));
		}
	}
	@media (min-width: 1200px) {
		.video-grid {
			grid-template-columns: repeat(4, minmax(0, 1fr));
		}
	}
	@media (max-width: 479px) {
		.library-toolbar {
			flex-wrap: wrap;
			margin-bottom: 18px;
		}
		.watch-filters {
			width: 100%;
			justify-content: space-between;
		}
		.watch-filters button {
			padding: 8px 12px;
		}
		.sort-control {
			margin-left: auto;
		}
		.video-grid {
			gap: 22px 12px;
		}
	}
</style>
