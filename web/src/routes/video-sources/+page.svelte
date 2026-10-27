<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/stores';
	onMount(() => {
		const platform = $page.url.searchParams.get('platform') === 'douyin' ? 'douyin' : 'bilibili';
		void goto('/platforms/' + platform + '?view=sources', { replaceState: true });
	});
</script>

<p role="status">正在打开平台收藏来源…</p>
