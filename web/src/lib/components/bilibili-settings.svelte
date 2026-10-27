<script lang="ts">
	import { onMount } from 'svelte';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Input } from '$lib/components/ui/input/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	import { Switch } from '$lib/components/ui/switch/index.js';
	import { Separator } from '$lib/components/ui/separator/index.js';
	import * as Tabs from '$lib/components/ui/tabs/index.js';
	import * as Tooltip from '$lib/components/ui/tooltip/index.js';
	import InfoIcon from '@lucide/svelte/icons/info';
	import FilterOptionEditor from '$lib/components/filter-option-editor.svelte';
	import api from '$lib/api';
	import { toast } from 'svelte-sonner';
	import type { Config, ApiError, DanmakuUpdateMilestone } from '$lib/types';
	let formData: Config | null = null;
	let loading = true;
	let saving = false;
	async function loadConfig() {
		loading = true;
		try {
			formData = (await api.getConfig()).data;
		} catch (error) {
			toast.error('加载 B站下载设置失败', { description: (error as ApiError).message });
		} finally {
			loading = false;
		}
	}
	async function saveConfig() {
		if (!formData || saving) return;
		saving = true;
		try {
			formData = (await api.updateConfig(formData)).data;
			toast.success('B站下载设置已保存');
		} catch (error) {
			toast.error('保存失败，请重新加载后再试', { description: (error as ApiError).message });
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

	onMount(() => {
		void loadConfig();
	});
</script>

{#if loading}<p role="status">正在加载 B站下载设置…</p>
{:else if formData}
	<Tabs.Root value="basic"
		><Tabs.List class="grid w-full grid-cols-4">
			<Tabs.Trigger value="basic">文件保存</Tabs.Trigger><Tabs.Trigger value="filter"
				>视频处理</Tabs.Trigger
			><Tabs.Trigger value="danmaku">弹幕渲染</Tabs.Trigger><Tabs.Trigger value="advanced"
				>高级设置</Tabs.Trigger
			></Tabs.List
		>
		<Tabs.Content value="basic" class="mt-6 space-y-6">
			<div class="grid grid-cols-1 gap-6 lg:grid-cols-2">
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
					<Input id="danmaku-font" placeholder="黑体" bind:value={formData.danmaku_option.font} />
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
					<Switch id="download-enable" bind:checked={formData.concurrent_limit.download.enable} />
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
	<div class="flex flex-wrap justify-end gap-3">
		<Button variant="outline" onclick={loadConfig} disabled={saving}>重新加载设置</Button><Button
			onclick={saveConfig}
			disabled={saving}>{saving ? '保存中…' : '保存 B站下载设置'}</Button
		>
	</div>
{:else}<p role="alert">B站下载设置加载失败。</p>
	<Button variant="outline" onclick={loadConfig}>重新加载设置</Button>{/if}
