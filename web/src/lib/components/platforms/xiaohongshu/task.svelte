<script lang="ts">
	import api from '$lib/api';
	import { xiaohongshuMessage } from '$lib/platform';
	import SyncTask from '../sync-task.svelte';
</script>

<SyncTask
	platform="xiaohongshu"
	name="小红书"
	getStatus={api.getXiaohongshuStatus}
	updateInterval={api.updateXiaohongshuConfig}
	trigger={api.triggerXiaohongshuTask}
	resume={api.resumeXiaohongshu}
	message={xiaohongshuMessage}
	stages={{
		preparing: '准备中',
		restoring: '恢复会话与验证账号',
		qr_preparing: '准备二维码',
		qr_ready: '等待扫码确认',
		folders: '获取收藏夹列表',
		scanning: '检查前 50 个不同条目',
		downloading: '下载与校验视频',
		registering: '登记已保存视频',
		qr_waiting: '等待手机扫码确认'
	}}
	insertedLabel="新发现视频入库"
	syncLabel="同步并重试失败项"
	retryLabel="同步并重试失败项"
	summaryNote="每轮检查页面前 50 个不同条目（含图文），统计仅计算已提交结果。窗口外新增可能遗漏；剩余待处理不含达到四次失败上限的条目。"
/>
