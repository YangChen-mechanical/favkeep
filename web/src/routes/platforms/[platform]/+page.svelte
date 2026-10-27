<script lang="ts">
	import { page } from '$app/stores';
	import { goto } from '$app/navigation';
	import { setBreadcrumb } from '$lib/stores/breadcrumb';
	import { Button } from '$lib/components/ui/button/index.js';
	import * as Tabs from '$lib/components/ui/tabs/index.js';
	import PlatformAccount from '$lib/components/platform-account.svelte';
	import PlatformSources from '$lib/components/platform-sources.svelte';
	import PlatformTask from '$lib/components/platform-task.svelte';
	import BilibiliSettings from '$lib/components/bilibili-settings.svelte';
	import type { Platform } from '$lib/types';

	const steps = [
		{ id: 'account', label: '连接账号' },
		{ id: 'sources', label: '收藏来源' },
		{ id: 'sync', label: '同步任务' }
	];
	$: platform = $page.params.platform as Platform;
	$: views = platform === 'bilibili' ? [...steps, { id: 'settings', label: '下载设置' }] : steps;
	$: supported = platform === 'bilibili' || platform === 'douyin';
	$: name = platform === 'douyin' ? '抖音' : 'B站';
	$: requested = $page.url.searchParams.get('view');
	$: view = views.some((item) => item.id === requested) ? requested! : 'account';
	$: setBreadcrumb([{ label: supported ? name : '平台不存在' }]);

	function changeView(value: string) {
		if (value !== view) void goto(`/platforms/${platform}?view=${value}`, { keepFocus: true });
	}
</script>

<svelte:head><title>{supported ? name : '平台不存在'} - favkeep</title></svelte:head>

{#if supported}
	<div class="space-y-6">
		<div class="flex flex-wrap items-center justify-between gap-3">
			<div>
				<h1 class="text-2xl font-semibold">{name}</h1>
				<p class="text-muted-foreground mt-1 text-sm">连接账号，选择收藏来源，然后查看同步结果。</p>
			</div>
			<Button variant="outline" href={`/watch?platform=${platform}`}>查看已保存视频</Button>
		</div>
		<Tabs.Root value={view} onValueChange={changeView}>
			<Tabs.List
				class={platform === 'bilibili'
					? 'grid h-auto w-full grid-cols-2 sm:grid-cols-4'
					: 'grid w-full grid-cols-3'}
				aria-label={`${name}管理步骤`}
			>
				{#each views as item, index (item.id)}
					<Tabs.Trigger value={item.id}>{index + 1}. {item.label}</Tabs.Trigger>
				{/each}
			</Tabs.List>
			{#each views as item (item.id)}
				<Tabs.Content value={item.id} class="mt-6 space-y-6">
					{#key platform}
						{#if view === item.id}
							{#if item.id === 'account'}<PlatformAccount {platform} />
							{:else if item.id === 'sources'}<PlatformSources {platform} />
							{:else if item.id === 'sync'}<PlatformTask {platform} />{:else}<BilibiliSettings
								/>{/if}
						{/if}
					{/key}
					<div class="flex flex-wrap justify-between gap-3 border-t pt-4">
						{#if view !== 'account'}<Button
								variant="outline"
								href={`/platforms/${platform}?view=account`}>连接账号</Button
							>{/if}
						{#if view === 'account'}<Button href={`/platforms/${platform}?view=sources`}
								>下一步：选择收藏来源</Button
							>
						{:else if view === 'sources'}<Button href={`/platforms/${platform}?view=sync`}
								>下一步：查看同步任务</Button
							>{/if}
					</div>
				</Tabs.Content>
			{/each}
		</Tabs.Root>
	</div>
{:else}
	<p role="alert">平台不存在，请从左侧选择 B站或抖音。</p>
{/if}
