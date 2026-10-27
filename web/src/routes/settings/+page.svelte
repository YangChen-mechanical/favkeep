<script lang="ts">
	import { douyinMessage } from '$lib/platform';
	import { onMount, tick } from 'svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Input } from '$lib/components/ui/input/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	import { Switch } from '$lib/components/ui/switch/index.js';
	import * as Tabs from '$lib/components/ui/tabs/index.js';
	import { Separator } from '$lib/components/ui/separator/index.js';
	import { Badge } from '$lib/components/ui/badge/index.js';
	import * as Dialog from '$lib/components/ui/dialog/index.js';
	import * as Tooltip from '$lib/components/ui/tooltip/index.js';
	import PasswordInput from '$lib/components/custom/password-input.svelte';
	import QrLogin from '$lib/components/custom/qr-login.svelte';
	import FilterOptionEditor from '$lib/components/filter-option-editor.svelte';
	import NotifierDialog from './NotifierDialog.svelte';
	import InfoIcon from '@lucide/svelte/icons/info';
	import QrCodeIcon from '@lucide/svelte/icons/qr-code';
	import api from '$lib/api';
	import { toast } from 'svelte-sonner';
	import { setBreadcrumb } from '$lib/stores/breadcrumb';
	import type {
		Config,
		ApiError,
		Notifier,
		Credential,
		DanmakuUpdateMilestone,
		DouyinStatus,
		Platform
	} from '$lib/types';

	let credential: Credential = {
		sessdata: '',
		bili_jct: '',
		buvid3: '',
		dedeuserid: '',
		ac_time_value: ''
	};
	let notifiers: Notifier[] = [];
	let notifierSaving = false;
	let credentialSaving = false;
	let currentPassword = '';
	let newPassword = '';
	let confirmPassword = '';
	let changingPassword = false;
	let config: Config | null = null;
	let formData: Config | null = null;
	let saving = false;
	let loading = false;

	let intervalInput: string = '1200';

	// Notifier 管理相关
	let showNotifierDialog = false;
	let editingNotifier: Notifier | null = null;
	let editingNotifierIndex: number | null = null;
	let isEditing = false;

	// QR 登录 Dialog 相关
	let showQrLoginDialog = false;
	let qrLoginComponent: QrLogin;
	let qrPlatform: Platform = 'bilibili';
	let douyin: DouyinStatus | null = null;
	let douyinError = '';
	let douyinInterval = 1800;
	let douyinSaving = false;

	async function loadDouyin() {
		douyinError = '';
		try {
			douyin = (await api.getDouyinStatus()).data;
			douyinInterval = douyin.interval;
		} catch (error) {
			douyinError = (error as ApiError).message;
		}
	}

	async function saveDouyinInterval() {
		if (!Number.isSafeInteger(douyinInterval) || douyinInterval <= 0) {
			toast.error('抖音同步间隔须为正整数秒');
			return;
		}
		douyinSaving = true;
		try {
			douyin = (await api.updateDouyinConfig(douyinInterval)).data;
			const current = (await api.getConfig()).data;
			if (formData && current.version === formData.version + 1) formData.version = current.version;
			toast.success('抖音同步间隔已保存');
		} catch (error) {
			toast.error('保存抖音同步间隔失败', { description: (error as ApiError).message });
		} finally {
			douyinSaving = false;
		}
	}

	async function openQrLogin(platform: Platform) {
		qrPlatform = platform;
		showQrLoginDialog = true;
		await tick();
		qrLoginComponent.init();
	}

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
	async function saveCredential() {
		if (!formData || credentialSaving) return;
		credentialSaving = true;
		try {
			const response = await api.updateCredential(formData.version, credential);
			formData.version = response.data.version;
			formData.credential_configured = response.data.credential_configured;
			credential = { sessdata: '', bili_jct: '', buvid3: '', dedeuserid: '', ac_time_value: '' };
			toast.success('B 站凭据已保存');
		} catch (error) {
			toast.error((error as ApiError).message);
		} finally {
			credentialSaving = false;
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

			// 根据 interval 的类型初始化输入框
			if (typeof formData.interval === 'number') {
				intervalInput = String(formData.interval);
			} else {
				intervalInput = formData.interval;
			}
		} catch (error) {
			console.error('加载配置失败:', error);
			toast.error('加载配置失败', {
				description: (error as ApiError).message
			});
			throw error;
		} finally {
			loading = false;
		}
	}

	async function saveConfig() {
		if (!formData) {
			toast.error('配置未加载');
			return;
		}

		// 保存前根据输入内容判断类型
		const trimmed = intervalInput.trim();
		const asNumber = Number(trimmed);

		if (!isNaN(asNumber) && trimmed !== '') {
			// 纯数字，作为 Interval
			formData.interval = asNumber;
		} else {
			// 非数字，作为 Cron 表达式
			formData.interval = trimmed;
		}

		saving = true;
		try {
			let resp = await api.updateConfig(formData);
			formData = resp.data;
			config = { ...formData };

			// 更新输入框显示
			if (typeof formData.interval === 'number') {
				intervalInput = String(formData.interval);
			} else {
				intervalInput = formData.interval;
			}

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

	function addDanmakuUpdateMilestone() {
		if (!formData) return;
		const milestones = formData.danmaku_update_policy.milestones;
		const last = milestones[milestones.length - 1];
		formData.danmaku_update_policy.milestones = [
			...milestones,
			{
				type: 'once',
				at_days: last ? danmakuUpdateMilestoneEndDays(last) + 30 : 3
			}
		];
	}

	function danmakuUpdateMilestoneEndDays(milestone: DanmakuUpdateMilestone) {
		return milestone.type === 'once' ? milestone.at_days : milestone.until_days;
	}

	function changeDanmakuUpdateMilestoneType(index: number, type: 'once' | 'periodic') {
		if (!formData) return;
		const milestones = [...formData.danmaku_update_policy.milestones];
		const current = milestones[index];
		const endDays = danmakuUpdateMilestoneEndDays(current);
		milestones[index] =
			type === 'once'
				? { type, at_days: endDays }
				: { type, until_days: endDays, interval_hours: 24 };
		formData.danmaku_update_policy.milestones = milestones;
	}

	function removeDanmakuUpdateMilestone(index: number) {
		if (!formData) return;
		formData.danmaku_update_policy.milestones = formData.danmaku_update_policy.milestones.filter(
			(_, milestoneIndex) => milestoneIndex !== index
		);
	}

	async function handleQrLoginSuccess() {
		showQrLoginDialog = false;
		if (qrPlatform === 'douyin') {
			await loadDouyin();
			toast.success('抖音已连接，正在刷新收藏夹');
			try {
				await api.refreshDouyinFolders();
			} catch (error) {
				toast.error('收藏夹刷新失败，请到视频源页面重试', {
					description: (error as ApiError).message
				});
			}
			return;
		}
		const response = await api.getConfig();
		if (formData && response.data.version === formData.version + 1) {
			formData.version = response.data.version;
			formData.credential_configured = response.data.credential_configured;
		} else {
			toast.info('B 站凭据已保存。配置已变化，保存普通设置前请重新加载。');
		}
		credential = { sessdata: '', bili_jct: '', buvid3: '', dedeuserid: '', ac_time_value: '' };
		toast.success('扫码登录成功，凭据已由服务器保存');
	}

	onMount(() => {
		setBreadcrumb([{ label: '设置' }]);
		loadConfig();
		void loadDouyin();
	});
</script>

<svelte:head>
	<title>设置 - Bili Sync</title>
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
				<Tabs.List class="grid w-full grid-cols-6">
					<Tabs.Trigger value="basic">基本设置</Tabs.Trigger>
					<Tabs.Trigger value="auth">平台连接</Tabs.Trigger>
					<Tabs.Trigger value="filter">视频处理</Tabs.Trigger>
					<Tabs.Trigger value="danmaku">弹幕渲染</Tabs.Trigger>
					<Tabs.Trigger value="notifiers">通知设置</Tabs.Trigger>
					<Tabs.Trigger value="advanced">高级设置</Tabs.Trigger>
				</Tabs.List>

				<!-- 基本设置 -->
				<Tabs.Content value="basic" class="mt-6 space-y-6">
					<div class="grid grid-cols-1 gap-6 lg:grid-cols-2">
						<div class="space-y-2">
							<Label for="bind-address">绑定地址</Label>
							<Input
								id="bind-address"
								placeholder="127.0.0.1:9999"
								bind:value={formData.bind_address}
							/>
						</div>
						<div class="space-y-2">
							<div class="flex items-center gap-1">
								<Label for="interval">任务触发条件</Label>
								<Tooltip.Root>
									<Tooltip.Trigger>
										<InfoIcon class="text-muted-foreground h-3.5 w-3.5" />
									</Tooltip.Trigger>
									<Tooltip.Content>
										<p class="text-xs">
											视频下载任务的触发条件，支持两种格式：<br />
											1. 输入数字表示间隔秒数，例如 1200 表示每隔 20 分钟触发一次； <br />
											2. 输入 Cron 表达式，格式为“秒 分 时 日 月 周”，例如“0 0 2 * * *”表示每天凌晨2点触发一次。
										</p>
									</Tooltip.Content>
								</Tooltip.Root>
							</div>
							<Input
								id="interval"
								type="text"
								bind:value={intervalInput}
								placeholder="1200 或 0 0 2 * * *"
							/>
						</div>
						<div class="space-y-2">
							<Label for="video-name">视频名称模板</Label>
							<Input id="video-name" bind:value={formData.video_name} />
						</div>
						<div class="space-y-2">
							<Label for="page-name">分页名称模板</Label>
							<Input id="page-name" bind:value={formData.page_name} />
						</div>
						<div class="space-y-2">
							<Label for="upper-path">UP主头像保存路径</Label>
							<Input
								id="upper-path"
								placeholder="/path/to/upper/faces"
								bind:value={formData.upper_path}
							/>
						</div>
						<div class="space-y-2">
							<Label for="time-format">时间格式</Label>
							<Input
								id="time-format"
								placeholder="%Y-%m-%d %H:%M:%S"
								bind:value={formData.time_format}
							/>
						</div>
					</div>

					<Separator />

					<div class="grid grid-cols-1 gap-6 lg:grid-cols-2">
						<div class="space-y-2">
							<Label for="favorite-default-path">收藏夹快捷订阅路径模板</Label>
							<Input id="favorite-default-path" bind:value={formData.favorite_default_path} />
						</div>
						<div class="space-y-2">
							<Label for="collection-default-path">合集快捷订阅路径模板</Label>
							<Input id="collection-default-path" bind:value={formData.collection_default_path} />
						</div>
						<div class="space-y-2">
							<Label for="submission-default-path">UP 主投稿快捷订阅路径模板</Label>
							<Input id="submission-default-path" bind:value={formData.submission_default_path} />
						</div>
					</div>

					<Separator />

					<div class="space-y-4">
						<div class="flex items-center space-x-2">
							<Switch id="cdn-sorting" bind:checked={formData.cdn_sorting} />
							<Label for="cdn-sorting">启用CDN排序</Label>
						</div>
						<div class="flex items-center space-x-2">
							<Switch id="try-upower-anyway" bind:checked={formData.try_upower_anyway} />
							<div class="flex items-center gap-1">
								<Label for="try-upower-anyway">尝试下载未充电视频</Label>
								<Tooltip.Root>
									<Tooltip.Trigger>
										<InfoIcon class="text-muted-foreground h-3.5 w-3.5" />
									</Tooltip.Trigger>
									<Tooltip.Content>
										<p class="text-xs">
											当关闭该开关时，程序仅会下载已充电的视频，未充电的视频直接跳过；开启后不再检查充电状态，一律尝试下载。<br
											/>
											这可以帮助下载未充电视频的封面等元数据，也应该可以下载未充电视频的试看部分（如果存在的话）。
										</p>
									</Tooltip.Content>
								</Tooltip.Root>
							</div>
						</div>
					</div>
				</Tabs.Content>

				<!-- B站认证 -->
				<Tabs.Content value="auth" class="mt-6 space-y-6">
					<section class="space-y-4 rounded-lg border p-4" aria-label="抖音连接">
						<div class="flex flex-wrap items-center justify-between gap-3">
							<div>
								<h2 class="font-semibold">抖音连接</h2>
								<p class="text-muted-foreground mt-1 text-sm">
									单账号连接；登录信息仅保存在服务器。
								</p>
							</div>
							<Button onclick={() => openQrLogin('douyin')}
								><QrCodeIcon class="mr-2 h-4 w-4" />{douyin?.connected
									? '重新登录'
									: '连接抖音'}</Button
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
									{douyinMessage(douyin.paused_reason)}。处理后请到仪表盘验证连接并恢复任务。
								</p>{/if}
						{:else if douyinError}
							<p class="text-destructive text-sm" role="alert">连接状态加载失败：{douyinError}</p>
							<Button variant="outline" size="sm" onclick={loadDouyin}>重试</Button>
						{:else}<p class="text-muted-foreground text-sm">正在加载连接状态…</p>{/if}
						<div class="flex flex-wrap items-end gap-3">
							<div class="space-y-2">
								<Label for="douyin-interval">抖音同步间隔（秒）</Label><Input
									id="douyin-interval"
									type="number"
									min="1"
									step="1"
									bind:value={douyinInterval}
									class="w-44"
								/>
							</div>
							<Button
								variant="outline"
								onclick={saveDouyinInterval}
								disabled={!douyin || douyinSaving}
								>{douyinSaving ? '保存中…' : '保存抖音间隔'}</Button
							>
						</div>
						<p class="text-muted-foreground text-xs">
							默认 1800 秒，独立于 B 站任务。连接后在<a class="underline" href="/video-sources"
								>视频源</a
							>勾选需要保存的收藏夹。
						</p>
					</section>
					<h2 class="font-semibold">B站连接</h2>
					<p class="text-muted-foreground text-sm">
						{formData.credential_configured
							? 'B 站凭据已配置。为保护账号，此处不回显；普通设置保存会保留原凭据。'
							: '尚未配置 B 站凭据。'}
					</p>
					<div class="flex items-center justify-between">
						<div class="space-y-1">
							<Label class="text-base font-semibold">快速登录</Label>
							<p class="text-muted-foreground text-sm">
								使用哔哩哔哩 APP 扫码登录，凭据仅保存在服务器
							</p>
						</div>
						<Button onclick={() => openQrLogin('bilibili')}>
							<QrCodeIcon class="mr-2 h-4 w-4" />
							扫码登录
						</Button>
					</div>

					<Separator />

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
							<PasswordInput
								id="buvid3"
								placeholder="请输入buvid3"
								bind:value={credential.buvid3}
							/>
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
				</Tabs.Content>

				<!-- 过滤规则 -->
				<Tabs.Content value="filter" class="mt-6 space-y-6">
					<FilterOptionEditor bind:value={formData.filter_option} />

					<Separator />

					<div class="space-y-4">
						<div class="flex items-center justify-between">
							<div class="space-y-1">
								<Label for="danmaku-update-enabled">弹幕自动更新</Label>
								<p class="text-muted-foreground text-sm">
									例如：希望第 3 天更新一次，之后每 7 天更新一次直到第 30 天，可添加“单次：3
									天”和“周期：截止 30 天，间隔 168 小时”。所有时间均从视频发布时间起算
								</p>
							</div>
							<Switch
								id="danmaku-update-enabled"
								bind:checked={formData.danmaku_update_policy.enabled}
							/>
						</div>

						<div class="space-y-3">
							{#each formData.danmaku_update_policy.milestones as milestone, index (index)}
								<div class="grid grid-cols-1 items-end gap-3 md:grid-cols-[9rem_1fr_1fr_auto]">
									<div class="space-y-2">
										<Label for={`danmaku-milestone-type-${index}`}>类型</Label>
										<select
											id={`danmaku-milestone-type-${index}`}
											class="border-input bg-background ring-offset-background focus-visible:ring-ring flex h-10 w-full rounded-md border px-3 py-2 text-sm focus-visible:ring-2 focus-visible:ring-offset-2 focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-50"
											disabled={!formData.danmaku_update_policy.enabled}
											value={milestone.type}
											onchange={(event) =>
												changeDanmakuUpdateMilestoneType(
													index,
													event.currentTarget.value as 'once' | 'periodic'
												)}
										>
											<option value="once">单次</option>
											<option value="periodic">周期</option>
										</select>
									</div>
									{#if milestone.type === 'once'}
										<div class="space-y-2 md:col-span-2">
											<Label for={`danmaku-milestone-at-${index}`}>发布时间后第几天</Label>
											<Input
												id={`danmaku-milestone-at-${index}`}
												type="number"
												min="1"
												disabled={!formData.danmaku_update_policy.enabled}
												bind:value={milestone.at_days}
											/>
										</div>
									{:else}
										<div class="space-y-2">
											<Label for={`danmaku-milestone-until-${index}`}>截止天数</Label>
											<Input
												id={`danmaku-milestone-until-${index}`}
												type="number"
												min="1"
												disabled={!formData.danmaku_update_policy.enabled}
												bind:value={milestone.until_days}
											/>
										</div>
										<div class="space-y-2">
											<Label for={`danmaku-milestone-interval-${index}`}>刷新间隔（小时）</Label>
											<Input
												id={`danmaku-milestone-interval-${index}`}
												type="number"
												min="1"
												disabled={!formData.danmaku_update_policy.enabled}
												bind:value={milestone.interval_hours}
											/>
										</div>
									{/if}
									<Button
										variant="outline"
										disabled={!formData.danmaku_update_policy.enabled}
										onclick={() => removeDanmakuUpdateMilestone(index)}>删除</Button
									>
								</div>
							{/each}
							<Button
								variant="outline"
								disabled={!formData.danmaku_update_policy.enabled}
								onclick={addDanmakuUpdateMilestone}>添加里程碑</Button
							>
						</div>
					</div>

					<Separator />

					<div class="space-y-4">
						<Label>处理跳过选项</Label>
						<p class="text-muted-foreground text-sm">在视频处理部分跳过某些执行环节</p>
						<div class="flex items-center space-x-2">
							<Switch id="skip-poster" bind:checked={formData.skip_option.no_poster} />
							<Label for="skip-poster">跳过视频封面</Label>
						</div>
						<div class="flex items-center space-x-2">
							<Switch id="skip-video-nfo" bind:checked={formData.skip_option.no_video_nfo} />
							<Label for="skip-video-nfo">跳过视频 NFO</Label>
						</div>
						<div class="flex items-center space-x-2">
							<Switch id="skip-upper-info" bind:checked={formData.skip_option.no_upper} />
							<Label for="skip-upper-info">跳过 Up 主头像、信息</Label>
						</div>
						<div class="flex items-center space-x-2">
							<Switch id="skip-danmaku" bind:checked={formData.skip_option.no_danmaku} />
							<Label for="skip-danmaku">跳过弹幕</Label>
						</div>
						<div class="flex items-center space-x-2">
							<Switch id="skip-subtitle" bind:checked={formData.skip_option.no_subtitle} />
							<Label for="skip-subtitle">跳过字幕</Label>
						</div>
					</div>
				</Tabs.Content>

				<!-- 弹幕设置 -->
				<Tabs.Content value="danmaku" class="mt-6 space-y-6">
					<div class="grid grid-cols-1 gap-6 lg:grid-cols-2">
						<div class="space-y-2">
							<Label for="danmaku-duration">弹幕持续时间（秒）</Label>
							<Input
								id="danmaku-duration"
								type="number"
								min="1"
								step="0.1"
								bind:value={formData.danmaku_option.duration}
							/>
						</div>
						<div class="space-y-2">
							<Label for="danmaku-font">字体</Label>
							<Input
								id="danmaku-font"
								placeholder="黑体"
								bind:value={formData.danmaku_option.font}
							/>
						</div>
						<div class="space-y-2">
							<Label for="danmaku-font-size">字体大小</Label>
							<Input
								id="danmaku-font-size"
								type="number"
								min="8"
								max="72"
								bind:value={formData.danmaku_option.font_size}
							/>
						</div>
						<div class="space-y-2">
							<Label for="danmaku-width-ratio">宽度比例</Label>
							<Input
								id="danmaku-width-ratio"
								type="number"
								min="0.1"
								max="3"
								step="0.1"
								bind:value={formData.danmaku_option.width_ratio}
							/>
						</div>
						<div class="space-y-2">
							<Label for="danmaku-horizontal-gap">水平间距</Label>
							<Input
								id="danmaku-horizontal-gap"
								type="number"
								min="0"
								step="1"
								bind:value={formData.danmaku_option.horizontal_gap}
							/>
						</div>
						<div class="space-y-2">
							<Label for="danmaku-lane-size">轨道大小</Label>
							<Input
								id="danmaku-lane-size"
								type="number"
								min="8"
								max="128"
								bind:value={formData.danmaku_option.lane_size}
							/>
						</div>
						<div class="space-y-2">
							<Label for="danmaku-float-percentage">滚动弹幕高度百分比</Label>
							<Input
								id="danmaku-float-percentage"
								type="number"
								min="0"
								max="1"
								step="0.01"
								bind:value={formData.danmaku_option.float_percentage}
							/>
						</div>
						<div class="space-y-2">
							<Label for="danmaku-bottom-percentage">底部弹幕高度百分比</Label>
							<Input
								id="danmaku-bottom-percentage"
								type="number"
								min="0"
								max="1"
								step="0.01"
								bind:value={formData.danmaku_option.bottom_percentage}
							/>
						</div>
						<div class="space-y-2">
							<Label for="danmaku-opacity">透明度 (0-255)</Label>
							<Input
								id="danmaku-opacity"
								type="number"
								min="0"
								max="255"
								bind:value={formData.danmaku_option.opacity}
							/>
						</div>
						<div class="space-y-2">
							<Label for="danmaku-outline">描边宽度</Label>
							<Input
								id="danmaku-outline"
								type="number"
								min="0"
								max="5"
								step="0.1"
								bind:value={formData.danmaku_option.outline}
							/>
						</div>
						<div class="space-y-2">
							<Label for="danmaku-time-offset">时间偏移（秒）</Label>
							<Input
								id="danmaku-time-offset"
								type="number"
								step="0.1"
								bind:value={formData.danmaku_option.time_offset}
							/>
						</div>
					</div>

					<Separator />

					<div class="space-y-4">
						<div class="flex items-center space-x-2">
							<Switch id="danmaku-bold" bind:checked={formData.danmaku_option.bold} />
							<Label for="danmaku-bold">粗体显示</Label>
						</div>
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
				<Tabs.Content value="advanced" class="mt-6 space-y-6">
					<div class="grid grid-cols-1 gap-6 lg:grid-cols-2">
						<div class="space-y-2">
							<Label for="video-concurrent">视频并发数</Label>
							<Input
								id="video-concurrent"
								type="number"
								min="1"
								max="20"
								bind:value={formData.concurrent_limit.video}
							/>
						</div>
						<div class="space-y-2">
							<Label for="page-concurrent">分页并发数</Label>
							<Input
								id="page-concurrent"
								type="number"
								min="1"
								max="20"
								bind:value={formData.concurrent_limit.page}
							/>
						</div>
						<div class="space-y-2">
							<Label for="nfo-time-type">NFO时间类型</Label>
							<select
								id="nfo-time-type"
								class="border-input bg-background ring-offset-background placeholder:text-muted-foreground focus-visible:ring-ring flex h-10 w-full rounded-md border px-3 py-2 text-sm file:border-0 file:bg-transparent file:text-sm file:font-medium focus-visible:ring-2 focus-visible:ring-offset-2 focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-50"
								bind:value={formData.nfo_time_type}
							>
								<option value="favtime">收藏时间</option>
								<option value="pubtime">发布时间</option>
							</select>
						</div>
					</div>

					<Separator />

					<div class="space-y-4">
						<div class="mb-4 flex items-center space-x-2">
							<Switch
								id="rate-limit-enable"
								checked={formData.concurrent_limit.rate_limit !== null &&
									formData.concurrent_limit.rate_limit !== undefined}
								onCheckedChange={(checked) => {
									if (checked) {
										formData!.concurrent_limit.rate_limit = { limit: 4, duration: 250 };
									} else {
										formData!.concurrent_limit.rate_limit = undefined;
									}
								}}
							/>
							<Label for="rate-limit-enable">启用请求频率限制</Label>
						</div>
						{#if formData.concurrent_limit.rate_limit}
							<div class="grid grid-cols-1 gap-6 lg:grid-cols-2">
								<div class="space-y-2">
									<Label for="rate-limit-duration">时间间隔（毫秒）</Label>
									<Input
										id="rate-limit-duration"
										type="number"
										min="100"
										bind:value={formData.concurrent_limit.rate_limit.duration}
									/>
									<p class="text-muted-foreground text-xs">请求限制的时间窗口（毫秒）</p>
								</div>
								<div class="space-y-2">
									<Label for="rate-limit-limit">限制请求数</Label>
									<Input
										id="rate-limit-limit"
										type="number"
										min="1"
										bind:value={formData.concurrent_limit.rate_limit.limit}
									/>
									<p class="text-muted-foreground text-xs">每个时间间隔内允许的最大请求数</p>
								</div>
							</div>
						{/if}
					</div>

					<Separator />
					<div class="space-y-4">
						<div class="mb-4 flex items-center space-x-2">
							<Switch
								id="download-enable"
								bind:checked={formData.concurrent_limit.download.enable}
							/>
							<Label for="rate-limit-duration">启用单文件分块下载</Label>
						</div>
						<div class="grid grid-cols-1 gap-6 lg:grid-cols-2">
							<div class="space-y-2">
								<Label for="download-concurrency">下载分块数</Label>
								<Input
									id="download-concurrency"
									type="number"
									min="1"
									max="16"
									disabled={!formData.concurrent_limit.download.enable}
									bind:value={formData.concurrent_limit.download.concurrency}
								/>
								<p class="text-muted-foreground text-xs">
									单文件将分为若干大小相同的块并行下载，所有分块下载完毕后合并
								</p>
							</div>
							<div class="space-y-2">
								<Label for="download-threshold">启用分块下载的文件大小阈值（字节）</Label>
								<Input
									id="download-threshold"
									type="number"
									min="1048576"
									disabled={!formData.concurrent_limit.download?.enable}
									bind:value={formData.concurrent_limit.download.threshold}
								/>
								<p class="text-muted-foreground text-xs">
									大于该阈值的文件才使用分块下载，文件过小时分块下载的拆分合并成本可能大于带来的增益
								</p>
							</div>
						</div>
					</div>
				</Tabs.Content>
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

<!-- QR 登录弹窗 -->
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
