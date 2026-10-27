<script lang="ts">
	import { onMount, tick } from 'svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';
	import * as Dialog from '$lib/components/ui/dialog/index.js';
	import PasswordInput from '$lib/components/custom/password-input.svelte';
	import QrLogin from '$lib/components/custom/qr-login.svelte';
	import QrCodeIcon from '@lucide/svelte/icons/qr-code';
	import api from '$lib/api';
	import { toast } from 'svelte-sonner';
	import { douyinMessage } from '$lib/platform';
	import type { ApiError, Credential, DouyinStatus, Platform } from '$lib/types';
	export let platform: Platform;
	let configured = false;
	let loaded = false;
	let loadError = '';
	let credential: Credential = {
		sessdata: '',
		bili_jct: '',
		buvid3: '',
		dedeuserid: '',
		ac_time_value: ''
	};
	let credentialSaving = false;
	let showQrLoginDialog = false;
	let qrLoginComponent: QrLogin;
	$: qrPlatform = platform;
	let douyin: DouyinStatus | null = null;
	let douyinError = '';
	async function loadAccount() {
		try {
			if (platform === 'douyin') douyin = (await api.getDouyinStatus()).data;
			else configured = (await api.getConfig()).data.credential_configured;
			loaded = true;
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
	async function saveCredential() {
		if (credentialSaving) return;
		credentialSaving = true;
		try {
			const config = (await api.getConfig()).data;
			const response = await api.updateCredential(config.version, credential);
			configured = response.data.credential_configured;
			credential = { sessdata: '', bili_jct: '', buvid3: '', dedeuserid: '', ac_time_value: '' };
			toast.success('B 站凭据已保存');
		} catch (error) {
			toast.error((error as ApiError).message);
		} finally {
			credentialSaving = false;
		}
	}
	async function handleQrLoginSuccess() {
		showQrLoginDialog = false;
		await loadAccount();
		if (platform === 'douyin') {
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

{#if platform === 'douyin'}
	<section class="space-y-4 rounded-lg border p-4" aria-label="抖音连接">
		<div class="flex flex-wrap items-center justify-between gap-3">
			<div>
				<h2 class="font-semibold">抖音连接</h2>
				<p class="text-muted-foreground mt-1 text-sm">单账号连接；登录信息仅保存在服务器。</p>
			</div>
			<Button onclick={openQrLogin}
				><QrCodeIcon class="mr-2 h-4 w-4" />{douyin?.connected ? '重新登录' : '连接抖音'}</Button
			>
		</div>
		{#if douyin}
			<div class="flex flex-wrap items-center gap-2 text-sm">
				<Badge variant={douyin.auth_state === 'connected' ? 'default' : 'secondary'}
					>{douyin.auth_state === 'connected'
						? '已连接'
						: douyin.auth_state === 'paused'
							? '已暂停'
							: '未连接'}</Badge
				>
				{#if douyin.account_name}<span>{douyin.account_name}</span>{/if}
			</div>
			{#if douyin.paused_reason}<p class="text-destructive text-sm" role="status">
					{douyinMessage(douyin.paused_reason)}。处理后请在“同步任务”验证连接并恢复。
				</p>{/if}
		{:else if douyinError}
			<p class="text-destructive text-sm" role="alert">连接状态加载失败：{douyinError}</p>
			<Button variant="outline" size="sm" onclick={loadDouyin}>重试</Button>
		{:else}<p class="text-muted-foreground text-sm">正在加载连接状态…</p>{/if}
		<p class="text-muted-foreground text-xs">
			连接后在<a class="underline" href="/platforms/douyin?view=sources">收藏来源</a
			>勾选需要保存的收藏夹。
		</p>
	</section>
{:else if loaded}
	<section class="space-y-4 rounded-lg border p-4" aria-label="B站连接">
		<h2 class="font-semibold">B站连接</h2>
		<p class="text-muted-foreground text-sm">
			{configured ? 'B 站凭据已配置。为保护账号，此处不回显。' : '尚未配置 B 站凭据。'}
		</p>
		<div class="flex items-center justify-between">
			<div class="space-y-1">
				<Label class="text-base font-semibold">快速登录</Label>
				<p class="text-muted-foreground text-sm">使用哔哩哔哩 APP 扫码登录，凭据仅保存在服务器</p>
			</div>
			<Button onclick={openQrLogin}>
				<QrCodeIcon class="mr-2 h-4 w-4" />
				扫码登录
			</Button>
		</div>

		<details class="space-y-4 rounded-lg border p-4">
			<summary class="cursor-pointer text-sm">手动替换登录凭据（高级）</summary>

			<Button onclick={saveCredential} disabled={credentialSaving}>保存 B 站凭据</Button>
			<!-- 手动替换完整 Cookie -->
			<div class="space-y-4">
				<div class="space-y-2">
					<Label for="sessdata">SESSDATA</Label>
					<PasswordInput
						id="sessdata"
						placeholder="请输入SESSDATA"
						bind:value={credential.sessdata}
					/>
				</div>
				<div class="space-y-2">
					<Label for="bili-jct">bili_jct</Label>
					<PasswordInput
						id="bili-jct"
						placeholder="请输入bili_jct"
						bind:value={credential.bili_jct}
					/>
				</div>
				<div class="space-y-2">
					<Label for="buvid3">buvid3</Label>
					<PasswordInput id="buvid3" placeholder="请输入buvid3" bind:value={credential.buvid3} />
				</div>
				<div class="space-y-2">
					<Label for="dedeuserid">dedeuserid</Label>
					<PasswordInput
						id="dedeuserid"
						placeholder="请输入dedeuserid"
						bind:value={credential.dedeuserid}
					/>
				</div>
				<div class="space-y-2">
					<Label for="ac-time-value">ac_time_value</Label>
					<PasswordInput
						id="ac-time-value"
						placeholder="请输入ac_time_value"
						bind:value={credential.ac_time_value}
					/>
				</div>
			</div>
		</details>
	</section>
{:else if loadError}<p role="alert">连接状态加载失败：{loadError}</p>
	<Button variant="outline" onclick={loadAccount}>重试</Button>
{:else}<p role="status">正在加载连接状态…</p>{/if}
<Dialog.Root bind:open={showQrLoginDialog}>
	<Dialog.Portal>
		<Dialog.Overlay class="bg-background/80 fixed inset-0 z-50 backdrop-blur-sm" />
		<Dialog.Content
			class="bg-background fixed top-[50%] left-[50%] z-50 grid w-full max-w-md translate-x-[-50%] translate-y-[-50%] gap-4 border p-6 shadow-lg duration-200 sm:rounded-lg"
		>
			<Dialog.Header>
				<Dialog.Title>扫码登录</Dialog.Title>
				<Dialog.Description
					>使用{qrPlatform === 'douyin' ? '抖音' : '哔哩哔哩'} APP 扫描二维码登录</Dialog.Description
				>
			</Dialog.Header>

			<QrLogin
				bind:this={qrLoginComponent}
				platform={qrPlatform}
				onSuccess={handleQrLoginSuccess}
			/>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
