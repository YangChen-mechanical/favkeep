<script lang="ts">
	import { page as routePage } from '$app/stores';
	import { Label } from '$lib/components/ui/label/index.js';
	import PagePlayer from './page-player.svelte';
	import type { PageInfo } from '$lib/types';

	export let pages: PageInfo[];
	let selectedId: number | null = null;
	$: downloaded = pages.filter((page) => page.download_status[1] === 7);
	$: requestedId = Number($routePage.url.searchParams.get('part'));
	$: if (downloaded.length && !downloaded.some((page) => page.id === selectedId)) {
		selectedId = downloaded.some((page) => page.id === requestedId)
			? requestedId
			: downloaded[0].id;
	}
	$: selected = downloaded.find((page) => page.id === selectedId) ?? downloaded[0];
</script>

<section class="mb-6 space-y-3" aria-label="本地播放">
	<h2 class="text-xl font-semibold">本地播放</h2>
	{#if downloaded.length === 0}
		<p class="text-muted-foreground">视频保存完成后即可在这里观看。</p>
	{:else if selected}
		{#if downloaded.length > 1}
			<Label for="local-player-part">选择分 P</Label>
			<select
				id="local-player-part"
				class="bg-background w-full rounded-md border p-2"
				bind:value={selectedId}
			>
				{#each downloaded as part (part.id)}
					<option value={part.id}>P{part.pid} · {part.name}</option>
				{/each}
			</select>
		{/if}
		{#key selected.id}
			<PagePlayer page={selected} />
		{/key}
	{/if}
</section>
