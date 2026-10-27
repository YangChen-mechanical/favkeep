<script lang="ts">
	import { onMount } from 'svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Input } from '$lib/components/ui/input/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	import { Switch } from '$lib/components/ui/switch/index.js';
	import { Separator } from '$lib/components/ui/separator/index.js';
	import * as Tabs from '$lib/components/ui/tabs/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';
	import * as Dialog from '$lib/components/ui/dialog/index.js';
	import NotifierDialog from './NotifierDialog.svelte';
	import api from '$lib/api';
	import { toast } from 'svelte-sonner';
	import { setBreadcrumb } from '$lib/stores/breadcrumb';
	import type { Config, ApiError, Notifier } from '$lib/types';

	let notifiers: Notifier[] = [];
	let notifierSaving = false;
	let currentPassword = '';
	let newPassword = '';
	let confirmPassword = '';
	let changingPassword = false;
	let config: Config | null = null;
	let formData: Config | null = null;
	let saving = false;
	let loading = false;

	// Notifier 管理相关
	let showNotifierDialog = false;
	let editingNotifier: Notifier | null = null;
	let editingNotifierIndex: number | null = null;
	let isEditing = false;

	function openAddNotifierDialog() {
		editingNotifier = null;
		editingNotifierIndex = null;
		isEditing = false;
		showNotifierDialog = true;
	}

	function openEditNotifierDialog(notifier: Notifier, index: number) {
		editingNotifier = { ...notifier };
		editingNotifierIndex = index;
		isEditing = true;
		showNotifierDialog = true;
	}

	function closeNotifierDialog() {
		showNotifierDialog = false;
		editingNotifier = null;
		editingNotifierIndex = null;
		isEditing = false;
	}

	async function persistNotifier(notifier: Notifier, index?: number) {
		if (!formData || notifierSaving) return;
		notifierSaving = true;
		try {
			const response = await api.saveNotifier(formData.version, notifier, index);
			notifiers = response.data.items;
			formData.version = response.data.version;
			closeNotifierDialog();
			toast.success('通知器已保存');
		} catch (error) {
			toast.error((error as ApiError).message);
		} finally {
			notifierSaving = false;
		}
	}
	function addNotifier(notifier: Notifier) {
		return persistNotifier(notifier);
	}
	function updateNotifier(index: number, notifier: Notifier) {
		return persistNotifier(notifier, index);
	}
	async function deleteNotifier(index: number) {
		if (!formData || notifierSaving || !window.confirm('删除此通知器？')) return;
		notifierSaving = true;
		try {
			const response = await api.deleteNotifier(formData.version, index);
			notifiers = response.data.items;
			formData.version = response.data.version;
			toast.success('通知器已删除');
		} catch (error) {
			toast.error((error as ApiError).message);
		} finally {
			notifierSaving = false;
		}
	}
	async function testNotifier(notifier: Notifier) {
		if (!formData || notifier.index === undefined) return;
		try {
			await api.testNotifier(formData.version, notifier.index);
			toast.success('测试通知发送成功');
		} catch (error) {
			toast.error((error as ApiError).message);
		}
	}
	async function changePassword(event: SubmitEvent) {
		event.preventDefault();
		if (changingPassword) return;
		if (newPassword !== confirmPassword) {
			toast.error('两次新密码不一致');
			return;
		}
		changingPassword = true;
		try {
			await api.changePassword(currentPassword, newPassword);
			currentPassword = '';
			newPassword = '';
			confirmPassword = '';
			toast.success('密码已修改，其他会话已退出');
		} catch (error) {
			toast.error((error as ApiError).message);
		} finally {
			changingPassword = false;
		}
	}

	async function loadConfig() {
		loading = true;
		try {
			const response = await api.getConfig();
			config = response.data;
			const notifications = await api.getNotifiers();
			if (notifications.data.version !== config.version)
				throw { message: '设置刚刚发生变化，请重新加载', status: 409 } as ApiError;
			notifiers = notifications.data.items;
			formData = { ...config };
		} catch (error) {
			console.error('加载配置失败:', error);
			toast.error('加载配置失败', {
				description: (error as ApiError).message
			});
		} finally {
			loading = false;
		}
	}

	async function saveConfig() {
		if (!formData) {
			toast.error('配置未加载');
			return;
		}

		saving = true;
		try {
			let resp = await api.updateConfig(formData);
			formData = resp.data;
			config = { ...formData };

			toast.success('配置已保存');
		} catch (error) {
			console.error('保存配置失败:', error);
			toast.error('保存配置失败', {
				description: (error as ApiError).message
			});
		} finally {
			saving = false;
		}
	}

	onMount(() => {
		setBreadcrumb([{ label: '通用设置' }]);
		loadConfig();
	});
</script>

<svelte:head>
	<title>通用设置 - favkeep</title>
</svelte:head>

<div class="space-y-6">
	<section class="bg-card space-y-4 rounded-lg border p-4" aria-label="访问与密码">
		<h2 class="font-semibold">访问与密码</h2>
		<p class="text-muted-foreground text-sm">
			观看和管理共用管理员登录。修改密码后，其他设备需重新登录。
		</p>
		<form onsubmit={changePassword} class="grid gap-4 sm:grid-cols-3">
			<div class="space-y-2">
				<Label for="current-password">当前密码</Label><Input
					id="current-password"
					type="password"
					bind:value={currentPassword}
					required
					autocomplete="current-password"
				/>
			</div>
			<div class="space-y-2">
				<Label for="new-password">新密码</Label><Input
					id="new-password"
					type="password"
					bind:value={newPassword}
					required
					minlength={15}
					maxlength={128}
					autocomplete="new-password"
				/>
			</div>
			<div class="space-y-2">
				<Label for="confirm-password">再次输入新密码</Label><Input
					id="confirm-password"
					type="password"
					bind:value={confirmPassword}
					required
					minlength={15}
					maxlength={128}
					autocomplete="new-password"
				/>
			</div>
			<Button type="submit" disabled={changingPassword}>修改密码</Button>
		</form>
	</section>
	<!-- 应用配置 -->
	{#if loading}
		<div class="flex items-center justify-center py-16">
			<div class="space-y-2 text-center">
				<div
					class="border-primary mx-auto h-6 w-6 animate-spin rounded-full border-2 border-t-transparent"
				></div>
				<p class="text-muted-foreground">加载配置中...</p>
			</div>
		</div>
	{:else if formData}
		<div class="space-y-6">
			<Tabs.Root value="basic" class="w-full">
				<Tabs.List class="grid h-auto w-full grid-cols-2">
					<Tabs.Trigger value="basic">服务设置</Tabs.Trigger>
					<Tabs.Trigger value="notifiers">通知设置</Tabs.Trigger>
				</Tabs.List>

				<!-- 基本设置 -->
				<Tabs.Content value="basic" class="mt-6 space-y-6"
					><div class="space-y-2">
						<Label for="bind-address">绑定地址</Label>
						<Input
							id="bind-address"
							placeholder="127.0.0.1:9999"
							bind:value={formData.bind_address}
						/>
					</div>
				</Tabs.Content>

				<!-- 通知设置 -->
				<Tabs.Content value="notifiers" class="mt-6 space-y-6">
					<div class="space-y-4">
						<div class="flex items-center justify-between">
							<div>
								<h3 class="text-lg font-semibold">通知器管理</h3>
								<p class="text-muted-foreground text-sm">
									配置通知器，在下载任务出现错误时发送通知
								</p>
							</div>
							<Button onclick={openAddNotifierDialog}>+ 添加通知器</Button>
						</div>

						{#if !notifiers || notifiers.length === 0}
							<div class="rounded-lg border-2 border-dashed py-12 text-center">
								<p class="text-muted-foreground">暂无通知器配置</p>
								<Button class="mt-4" variant="outline" onclick={openAddNotifierDialog}>
									添加第一个通知器
								</Button>
							</div>
						{:else}
							<div class="space-y-3">
								{#each notifiers as notifier, index (index)}
									<div class="flex items-center justify-between rounded-lg border p-4">
										<div class="flex-1">
											{#if notifier.type === 'telegram'}
												<div class="flex items-center gap-2">
													<Badge variant="secondary">Telegram</Badge>
													<span class="text-muted-foreground text-sm">
														Chat ID: {notifier.chat_id}
													</span>
												</div>
											{:else if notifier.type === 'webhook'}
												<div class="flex items-center gap-2">
													<Badge variant="secondary">Webhook</Badge>
													<span class="text-muted-foreground text-sm">
														{notifier.configured?.url ? 'URL 已配置' : 'URL 未配置'}
													</span>
												</div>
											{:else if notifier.type === 'smtp'}
												<div class="flex flex-wrap items-center gap-2">
													<Badge variant="secondary">SMTP</Badge>
													<span class="text-muted-foreground text-sm break-all">
														{notifier.host}:{notifier.port} → {notifier.to}
													</span>
												</div>
											{/if}
										</div>
										<div class="flex gap-2">
											<Button
												size="sm"
												variant="outline"
												onclick={() => openEditNotifierDialog(notifier, index)}
											>
												编辑
											</Button>
											<Button size="sm" variant="secondary" onclick={() => testNotifier(notifier)}>
												测试
											</Button>
											<Button size="sm" variant="destructive" onclick={() => deleteNotifier(index)}>
												删除
											</Button>
										</div>
									</div>
								{/each}
							</div>
						{/if}
					</div>

					<Separator />

					<div class="flex items-center justify-between rounded-lg border p-4">
						<div class="space-y-1">
							<Label for="ignore-common-errors">忽略常见错误</Label>
							<p class="text-muted-foreground text-sm">
								接口偶发 -503 服务暂不可用或 -504
								服务调用超时，在通知器内容易刷屏，开启该选项以忽略此类错误通知
							</p>
						</div>
						<Switch id="ignore-common-errors" bind:checked={formData.ignore_common_errors} />
					</div>
				</Tabs.Content>

				<!-- 高级设置 -->
			</Tabs.Root>

			<div class="flex justify-end pt-6">
				<Button onclick={saveConfig} disabled={saving} size="lg">
					{saving ? '保存中...' : '保存配置'}
				</Button>
			</div>
		</div>
	{:else}
		<div class="flex items-center justify-center py-16">
			<div class="space-y-4 text-center">
				<p class="text-muted-foreground">配置加载失败，请重试</p>
				<Button onclick={loadConfig}>重新加载</Button>
			</div>
		</div>
	{/if}
</div>

<Dialog.Root bind:open={showNotifierDialog}>
	<Dialog.Portal>
		<Dialog.Overlay class="bg-background/80 fixed inset-0 z-50 backdrop-blur-sm" />
		<Dialog.Content
			class="bg-background fixed top-[50%] left-[50%] z-50 grid max-h-[90vh] w-full max-w-lg translate-x-[-50%] translate-y-[-50%] gap-4 overflow-y-auto border p-6 shadow-lg duration-200 sm:rounded-lg"
		>
			<Dialog.Header>
				<Dialog.Title>
					{isEditing ? '编辑通知器' : '添加通知器'}
				</Dialog.Title>
				<Dialog.Description>配置通知器类型和参数</Dialog.Description>
			</Dialog.Header>

			{#if showNotifierDialog}
				<NotifierDialog
					notifier={editingNotifier}
					busy={notifierSaving}
					onSave={(notifier) => {
						if (isEditing && editingNotifierIndex !== null) {
							updateNotifier(editingNotifierIndex, notifier);
						} else {
							addNotifier(notifier);
						}
					}}
					onCancel={closeNotifierDialog}
				/>
			{/if}
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
