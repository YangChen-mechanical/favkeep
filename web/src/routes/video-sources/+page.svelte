<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/stores';
	import { platforms } from '$lib/stores/platforms';
	let unsupported = false;
	onMount(() =>
		platforms.subscribe((items) => {
			if (!items.length) return;
			const platform = $page.url.searchParams.get('platform') || 'bilibili';
			if (!items.some((item) => item.id === platform)) {
				unsupported = true;
				return;
			}
			void goto('/platforms/' + platform + '?view=sources', { replaceState: true });
		})
	);
</script>

{#if unsupported}<p role="alert">不支持此平台，请选择已接入的平台。</p>{:else}<p role="status">
		正在打开平台收藏来源…
	</p>{/if}
