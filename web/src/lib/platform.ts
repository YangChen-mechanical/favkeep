import type { Platform } from './types';

export const platformLabels: Record<Platform, string> = { bilibili: 'B站', douyin: '抖音' };

const douyinMessages: Record<string, string> = {
	rejected: '平台拒绝了请求，请等待冷却后重试',
	platform_rejected: '平台拒绝了请求，请等待冷却后重试',
	challenge: '需要完成平台验证',
	verification_required: '需要完成平台验证',
	login_expired: '登录已失效，请重新连接',
	identity_unverified: '未能确认账号身份，已暂停，请检查连接',
	account_changed: '账号不一致，已暂停，请使用原账号',
	disk_full: '磁盘空间不足，请清理后恢复',
	network: '网络连接失败，请稍后重试',
	network_failed: '网络连接失败，请稍后重试',
	failed: '同步未完成，请检查后重试',
	invalid_platform_data: '平台返回内容异常，本次同步未完成',
	incomplete_pagination: '收藏夹扫描未完成，本次未更新基线',
	browser_failed: '浏览器连接失败，请检查后重试',
	private_file_failed: '本地文件保存失败，请检查目录权限和磁盘空间',
	media_unavailable: '视频暂时无法下载',
	browser_token_unavailable: '未能确认连接，请重新连接后重试',
	selection_changed_during_scan: '收藏夹选择已变化，下次同步会按最新选择执行',
	account_busy: '抖音任务正在处理中，请稍候',
	account_paused_or_cooling_down: '任务已暂停或正在冷却，请稍后检查'
};

export function douyinMessage(code: string): string {
	return Object.hasOwn(douyinMessages, code) ? douyinMessages[code] : '需要检查';
}

export function sourceVideoUrl(video: {
	platform: Platform;
	platform_id: string | null;
	bvid?: string | null;
}): string | null {
	const id = video.platform === 'douyin' ? video.platform_id : video.bvid;
	if (!id) return null;
	return video.platform === 'douyin'
		? `https://www.douyin.com/video/${encodeURIComponent(id)}`
		: `https://www.bilibili.com/video/${encodeURIComponent(id)}/`;
}
