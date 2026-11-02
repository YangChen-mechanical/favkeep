import { defineConfig } from "vitepress";
import taskLists from "markdown-it-task-lists";

// https://vitepress.dev/reference/site-config
export default defineConfig({
 srcExclude: ["migration-from-bili-sync.md", "migration-result-*.md"],
	title: "favkeep",
	description: "个人视频归档与观看应用",
	lang: "zh-Hans",
	lastUpdated: true,
	cleanUrls: true,
	metaChunk: true,
	themeConfig: {
		outline: {
			label: "页面导航",
			level: "deep",
		},
		// https://vitepress.dev/reference/default-theme-config
		nav: [
			{ text: "主页", link: "/" },
			{
				text: "开发中",
				items: [
					{
						text: "项目源码",
						link: "https://github.com/YangChen-mechanical/favkeep",
					},
					{
						text: "文档更新",
						link: "https://github.com/YangChen-mechanical/favkeep/commits/favkeep/docs",
					},
				],
			},
		],
		sidebar: [
 { text: "favkeep", items: [
 { text: "快速开始", link: "/quick-start" },
 { text: "代码结构", link: "/architecture" },
 { text: "开发与检查", link: "/development/README" },
 { text: "开发路线", link: "/development/roadmap" },
 { text: "应用认证", link: "/operations/authentication" },
 { text: "媒体路径", link: "/operations/media-paths" },
 { text: "升级与回退", link: "/operations/deployment" },
 { text: "开源检查", link: "/development/open-source" },
 { text: "抖音", link: "/platforms/douyin" },
 { text: "小红书", link: "/platforms/xiaohongshu" },
 { text: "历史记录", link: "/archive/README" }
 ] },
			{
				text: "上游参考",
				items: [
					{ text: "什么是 bili-sync？", link: "/introduction" },
				],
			},
			{
				text: "细节",
				items: [
					{ text: "配置说明", link: "/configuration" },
					{ text: "命令行参数", link: "/args" },
					{ text: "工作原理", link: "/design" },
				],
			},
			{
				text: "参考",
				items: [
					{ text: "获取收藏夹信息", link: "/favorite" },
					{
						text: "获取合集/列表信息",
						link: "/collection",
					},
					{ text: "获取用户投稿信息", link: "/submission" },
				],
			},
			{
				text: "其它",
				items: [
					{ text: "常见问题", link: "/question" },
					{ text: "管理页", link: "/frontend" },
				],
			}
		],
		socialLinks: [
			{ icon: "github", link: "https://github.com/YangChen-mechanical/favkeep" },
		],
		search: {
			provider: "local",
		},
		notFound: {
			title: "你来到了没有知识的荒原",
			quote: "这里什么都没有",
			linkText: "返回首页",
		},
		docFooter: {
			prev: "上一页",
			next: "下一页",
		},
		lastUpdated: {
			text: "上次更新于",
		},
		returnToTopLabel: "回到顶部",
		sidebarMenuLabel: "菜单",
		darkModeSwitchLabel: "主题",
		lightModeSwitchTitle: "切换到浅色模式",
		darkModeSwitchTitle: "切换到深色模式",
	},
	markdown: {
		config: (md) => {
			md.use(taskLists);
		},
		theme: {
			light: "github-light",
			dark: "github-dark",
		},
	},
	head: [
		["link", { rel: "icon", type: "image/svg+xml", href: "/icon.svg" }],
	],
});
