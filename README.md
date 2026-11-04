# favkeep（开发中）

基于 [amtoaer/bili-sync](https://github.com/amtoaer/bili-sync) 的个人视频归档与观看应用，使用 Rust、Svelte 和 SQLite，面向个人自托管的单管理员实例。

支持 B 站、抖音和小红书的归档管理，以及本地 MP4 播放、分 P 切换、观看进度和继续观看。抖音与小红书使用 Rust 原生 HTTP，无需部署浏览器或签名服务；各平台的收藏范围和首次基线规则见平台文档。

## 安装与使用

当前按源码构建交付，尚未发布 favkeep 官方二进制或容器镜像。上游 bili-sync 的程序和镜像不包含 favkeep 的认证与观看功能。

1. 按[快速开始](docs/quick-start.md)安装工具链、构建前端及 Rust 程序。
2. 指定独立配置目录、媒体目录和 `FAVKEEP_APP_ORIGIN`，启动服务。
3. 从服务器配置目录读取 `auth-setup.secret`，访问 `/setup` 创建管理员，再在网页连接平台账号并配置来源。

访问 `/` 进入管理后台，访问 `/watch` 进入观看中心。API、媒体和 WebSocket 共用管理员会话；旧应用 token 已停止认证。

## 支持范围与限制

- 这是开发中的个人自托管应用，不支持多用户或多租户。现有验收包含 Windows GNU 与 Linux ARM64；其他平台及长期运行仍需验证。
- 抖音首次完整扫描已选收藏夹时建立基线并跳过历史；小红书处理最近 50 条收藏窗口。具体资格与恢复规则见[抖音](docs/platforms/douyin.md)、[小红书](docs/platforms/xiaohongshu.md)。
- 平台接口、扫码和风控可能变化；遇到验证或限流时需人工处理，不保证永久可用。
- 当前验收主要覆盖私有访问。公网部署需要 HTTPS、正确的 Origin 和反向代理配置；登录失败全局限速可能影响其他登录尝试，页面防嵌入响应头仍待完善。首版建议仅通过本机或受控私有网络访问。
- `--douyin-import-session` 存在 TLS 初始化顺序问题，当前请使用网页扫码登录。恢复及升级必须先备份，不能让旧程序直接连接已升级数据库。
- 平台凭据保存在服务器配置目录的数据库及账号文件中，未做应用层加密。不要上传配置目录、数据库、备份、Cookie、日志或初始化码。

## 文档与开发

[快速开始](docs/quick-start.md) · [应用认证与恢复](docs/operations/authentication.md) · [媒体目录](docs/operations/media-paths.md) · [升级与回退](docs/operations/deployment.md)

[代码结构](docs/architecture.md) · [开发与检查](docs/development/README.md) · [开发路线](docs/development/roadmap.md) · [支持范围与开源检查](docs/development/open-source.md)

## 授权与致谢

项目采用 [MIT](License) 许可证，保留 bili-sync 上游版权声明。移植代码的独立许可证、来源和修改范围见 [第三方说明](THIRD_PARTY_NOTICES.md)。依赖包继续适用各自许可证。

感谢 [bili-sync](https://github.com/amtoaer/bili-sync)、[bilibili-API-collect](https://github.com/SocialSisterYi/bilibili-API-collect)、[bilibili-api](https://github.com/Nemo2011/bilibili-api)、[danmu2ass](https://github.com/gwy15/danmu2ass)、[F2](https://github.com/Johnserf-Seed/f2)、[catbus](https://github.com/cv-cat/catbus) 和 [xhshow](https://github.com/Cloxl/xhshow)。

代码许可证不授予平台商标、视频、音乐或封面的使用权。请仅归档你有权保存的内容，遵守适用法律及平台条款；本项目与这些平台无隶属或背书关系。
