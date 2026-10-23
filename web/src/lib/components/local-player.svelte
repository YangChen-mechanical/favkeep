<script lang="ts">
	import { onMount } from 'svelte';
	import { page as routePage } from '$app/stores';
	import { Label } from '$lib/components/ui/label/index.js';
	import { Button } from '$lib/components/ui/button/index.js';
	import PagePlayer from './page-player.svelte';
	import api from '$lib/api';
	import type { ApiError, PageInfo } from '$lib/types';

	export let pages: PageInfo[];
	let selectedId: number | null = null;
	let ready = false;
	let error = '';
	$: downloaded = pages.filter((page) => page.download_status[1] === 7);
	$: requestedId = Number($routePage.url.searchParams.get('part'));
	$: if (downloaded.length && !downloaded.some((page) => page.id === selectedId)) {
		selectedId = downloaded.some((page) => page.id === requestedId)
			? requestedId
			: downloaded[0].id;
	}
	$: selected = downloaded.find((page) => page.id === selectedId) ?? downloaded[0];

	async function connect() {
		error = '';
		try {
			await api.openMediaSession();
			ready = true;
		} catch (cause) {
			error = (cause as ApiError).message;
		}
	}

	onMount(() => {
		void connect();
	});
</script>

<section class="mb-6 space-y-3" aria-label="本地播放">
	<h2 class="text-xl font-semibold">本地播放</h2>
	{#if downloaded.length === 0}
		<p class="text-muted-foreground">视频保存完成后即可在这里观看。</p>
	{:else if error}
		<p role="alert" class="text-destructive">无法准备播放：{error}</p>
		<Button variant="outline" onclick={connect}>重试</Button>
	{:else if ready && selected}
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
	{:else}
		<p class="text-muted-foreground" role="status">正在准备播放…</p>
	{/if}
</section>
