# favkeep

个人视频归档与观看应用，基于 bili-sync，使用 Rust、Svelte 和 SQLite。

当前支持 B 站、抖音和小红书，提供单管理员会话、归档管理、本地 MP4 播放、分 P、观看进度与继续观看。抖音与小红书使用 Rust 原生 HTTP；各平台的收藏来源与基线规则不同。

## 安装与使用

- [快速开始与首次启动](quick-start.md)
- [应用认证与密码恢复](operations/authentication.md)
- [配置说明](configuration.md)与[命令行参数](args.md)
- B 站来源：[收藏夹](favorite.md)、[合集与列表](collection.md)、[UP 主投稿](submission.md)
- [抖音](platforms/douyin.md)与[小红书](platforms/xiaohongshu.md)
- [管理页](frontend.md)与[常见问题](question.md)

## 部署与维护

- [媒体目录与旧路径兼容](operations/media-paths.md)
- [升级、备份与回退](operations/deployment.md)
- [支持范围与开源检查](development/open-source.md)

## 开发

- [代码结构与依赖方向](architecture.md)
- [开发环境、检查命令与工具](development/README.md)
- [开发路线](development/roadmap.md)
- [平台隔离原则](development/platform-isolation.md)

## 上游参考

上游的[简介](introduction.md)和[工作原理](design.md)作为 B 站能力参考；favkeep 的认证、支持范围与部署以当前文档为准。
