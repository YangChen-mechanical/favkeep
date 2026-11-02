<script lang="ts">
	import api from '$lib/api';
	import { douyinMessage } from '$lib/platform';
	import SyncTask from '../sync-task.svelte';
</script>

<SyncTask
	platform="douyin"
	name="抖音"
	getStatus={api.getDouyinStatus}
	updateInterval={api.updateDouyinConfig}
	trigger={api.triggerDouyinTask}
	resume={api.resumeDouyin}
	message={douyinMessage}
	stages={{
		preparing: '准备中',
		restoring: '恢复会话与验证账号',
		qr_preparing: '准备二维码',
		qr_ready: '等待扫码确认',
		folders: '获取收藏夹列表',
		scanning: '扫描收藏夹',
		downloading: '下载与校验视频',
		registering: '登记已保存视频'
	}}
	insertedLabel="新增入库"
	syncLabel="立即同步抖音"
	retryLabel="重试抖音同步"
	summaryNote="扫描条目按收藏夹累计，跨夹重复仅新增入库一次。剩余待处理仅含当前来源允许且未达到重试上限的条目。"
/>
