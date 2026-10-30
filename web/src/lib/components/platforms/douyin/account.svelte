<script lang="ts">
	import { onMount, tick } from 'svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';
	import * as Dialog from '$lib/components/ui/dialog/index.js';
	import QrLogin from '$lib/components/custom/qr-login.svelte';
	import QrCodeIcon from '@lucide/svelte/icons/qr-code';
	import api from '$lib/api';
	import { toast } from 'svelte-sonner';
	import { douyinMessage } from '$lib/platform';
	import type { ApiError, DouyinStatus } from '$lib/types';
	const platform = 'douyin';
	let loadError = '';
	let showQrLoginDialog = false;
	let qrLoginComponent: QrLogin;
	const qrPlatform = platform;
	let douyin: DouyinStatus | null = null;
	let douyinError = '';
	async function loadAccount() {
		try {
			douyin = (await api.getDouyinStatus()).data;
			loadError = '';
			douyinError = '';
		} catch (error) {
			loadError = (error as ApiError).message;
			douyinError = loadError;
		}
	}
	const loadDouyin = loadAccount;
	async function openQrLogin() {
		showQrLoginDialog = true;
		await tick();
		qrLoginComponent.init();
	}
	async function handleQrLoginSuccess() {
		showQrLoginDialog = false;
		await loadAccount();
		{
			try {
				await api.refreshDouyinFolders();
			} catch (error) {
				toast.error('收藏夹刷新失败，请在收藏来源重试', {
					description: (error as ApiError).message
				});
			}
		}
		toast.success('账号连接已保存');
	}
	onMount(() => {
		void loadAccount();
	});
</script>

<section class="space-y-4 rounded-lg border p-4" aria-label="抖音连接">
	<div class="flex flex-wrap items-center justify-between gap-3">
		<div>
			<h2 class="font-semibold">抖音连接</h2>
			<p class="text-muted-foreground mt-1 text-sm">单账号连接；登录信息仅保存在服务器。</p>
		</div>

		<Button onclick={openQrLogin}>
			<QrCodeIcon class="mr-2 h-4 w-4" />
			{douyin?.connected ? '重新登录' : '连接抖音'}
		</Button>
	</div>

	{#if douyin}
		<div class="flex flex-wrap items-center gap-2 text-sm">
			<Badge variant={douyin.auth_state === 'connected' ? 'default' : 'secondary'}>
				{douyin.auth_state === 'connected'
					? '已连接'
					: douyin.auth_state === 'paused'
						? '已暂停'
						: '未连接'}
			</Badge>

			{#if douyin.account_name}
				<span>{douyin.account_name}</span>
			{/if}
		</div>

		{#if douyin.paused_reason}
			<p class="text-destructive text-sm" role="status">
				{douyinMessage(douyin.paused_reason)}。处理后请在“同步任务”验证连接并恢复。
			</p>
		{/if}
	{:else if douyinError}
		<p class="text-destructive text-sm" role="alert">连接状态加载失败：{douyinError}</p>
		<Button variant="outline" size="sm" onclick={loadDouyin}>重试</Button>
	{:else}
		<p class="text-muted-foreground text-sm">正在加载连接状态…</p>
	{/if}

	<p class="text-muted-foreground text-xs">
		连接后在

		<a class="underline" href="/platforms/douyin?view=sources">收藏来源</a>

		勾选需要保存的收藏夹。
	</p>
</section>

<Dialog.Root bind:open={showQrLoginDialog}>
	<Dialog.Portal>
		<Dialog.Overlay class="bg-background/80 fixed inset-0 z-50 backdrop-blur-sm" />

		<Dialog.Content
			class="bg-background fixed top-[50%] left-[50%] z-50 grid w-full max-w-md translate-x-[-50%] translate-y-[-50%] gap-4 border p-6 shadow-lg duration-200 sm:rounded-lg"
		>
			<Dialog.Header>
				<Dialog.Title>扫码登录</Dialog.Title>
				<Dialog.Description>使用抖音 APP 扫描二维码登录</Dialog.Description>
			</Dialog.Header>

			<QrLogin
				bind:this={qrLoginComponent}
				platform={qrPlatform}
				onSuccess={handleQrLoginSuccess}
			/>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
