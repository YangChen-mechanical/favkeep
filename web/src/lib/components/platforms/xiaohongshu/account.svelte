<script lang="ts">
	import { onMount, tick } from 'svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';
	import * as Dialog from '$lib/components/ui/dialog/index.js';
	import QrLogin from '$lib/components/custom/qr-login.svelte';
	import QrCodeIcon from '@lucide/svelte/icons/qr-code';
	import api from '$lib/api';
	import { toast } from 'svelte-sonner';
	import { xiaohongshuMessage } from '$lib/platform';
	import type { ApiError, XiaohongshuStatus } from '$lib/types';
	const platform = 'xiaohongshu';
	let loadError = '';
	let showQrLoginDialog = false;
	let qrLoginComponent: QrLogin;
	const qrPlatform = platform;
	let xiaohongshu: XiaohongshuStatus | null = null;
	let xiaohongshuError = '';
	async function loadAccount() {
		try {
			xiaohongshu = (await api.getXiaohongshuStatus()).data;
			loadError = '';
			xiaohongshuError = '';
		} catch (error) {
			loadError = (error as ApiError).message;
			xiaohongshuError = loadError;
		}
	}
	const loadXiaohongshu = loadAccount;
	async function openQrLogin() {
		showQrLoginDialog = true;
		await tick();
		qrLoginComponent.init();
	}
	async function handleQrLoginSuccess() {
		showQrLoginDialog = false;
		await loadAccount();
		toast.success('账号连接已保存');
	}
	onMount(() => {
		void loadAccount();
	});
</script>

<section class="space-y-4 rounded-lg border p-4" aria-label="小红书连接">
	<div class="flex flex-wrap items-center justify-between gap-3">
		<div>
			<h2 class="font-semibold">小红书连接</h2>
			<p class="text-muted-foreground mt-1 text-sm">单账号连接；登录信息仅保存在服务器。</p>
		</div>

		<Button onclick={openQrLogin}>
			<QrCodeIcon class="mr-2 h-4 w-4" />
			{xiaohongshu?.connected ? '重新登录' : '连接小红书'}
		</Button>
	</div>

	{#if xiaohongshu}
		<div class="flex flex-wrap items-center gap-2 text-sm">
			<Badge variant={xiaohongshu.auth_state === 'connected' ? 'default' : 'secondary'}>
				{xiaohongshu.auth_state === 'connected'
					? '已连接'
					: xiaohongshu.auth_state === 'paused'
						? '已暂停'
						: '未连接'}
			</Badge>

			{#if xiaohongshu.account_name}
				<span>{xiaohongshu.account_name}</span>
			{/if}
		</div>

		{#if xiaohongshu.paused_reason}
			<p class="text-destructive text-sm" role="status">
				{xiaohongshuMessage(xiaohongshu.paused_reason)}。处理后请在“同步任务”验证连接并恢复。
			</p>
		{/if}
	{:else if xiaohongshuError}
		<p class="text-destructive text-sm" role="alert">连接状态加载失败：{xiaohongshuError}</p>
		<Button variant="outline" size="sm" onclick={loadXiaohongshu}>重试</Button>
	{:else}
		<p class="text-muted-foreground text-sm">正在加载连接状态…</p>
	{/if}

	<p class="text-muted-foreground text-xs">
		连接后在

		<a class="underline" href="/platforms/xiaohongshu?view=sources">收藏来源</a>

		启用“全部收藏”来源。
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
				<Dialog.Description>使用小红书 APP 扫描二维码登录</Dialog.Description>
			</Dialog.Header>

			<QrLogin
				bind:this={qrLoginComponent}
				platform={qrPlatform}
				onSuccess={handleQrLoginSuccess}
			/>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
