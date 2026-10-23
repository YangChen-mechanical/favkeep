<script lang="ts">
	import ImageIcon from '@lucide/svelte/icons/image';
	export let videoId: number;
	export let title: string;
	let failed = false;
	let loaded = false;
</script>

<div class="cover-image" class:loaded>
	{#if failed}
		<div class="missing" aria-label="封面暂缺"><ImageIcon size={28} /><span>封面暂缺</span></div>
	{:else}
		<img
			src="/api/media/videos/{videoId}/cover"
			alt={title}
			loading="lazy"
			decoding="async"
			onload={() => (loaded = true)}
			onerror={() => (failed = true)}
		/>
	{/if}
</div>

<style>
	.cover-image {
		width: 100%;
		height: 100%;
		background: var(--watch-placeholder);
		overflow: hidden;
	}
	img {
		width: 100%;
		height: 100%;
		object-fit: cover;
		display: block;
		opacity: 0;
		transition: opacity 0.18s;
	}
	.loaded img {
		opacity: 1;
	}
	.missing {
		height: 100%;
		display: flex;
		align-items: center;
		justify-content: center;
		gap: 8px;
		color: var(--watch-muted);
		font-size: 12px;
	}
	@media (prefers-reduced-motion: reduce) {
		img {
			transition: none;
		}
	}
</style>
