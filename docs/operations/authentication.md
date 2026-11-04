# favkeep 应用认证

应用使用单管理员用户名、密码和服务端会话。API、观看区、媒体 GET/HEAD/Range、封面及 WebSocket 共用此认证；B 站账号连接仍是独立的平台凭据。

## 启动与首次设置

设置 `FAVKEEP_APP_ORIGIN` 为浏览器实际使用的完整 Origin，例如 `https://favkeep.example.com`，不加末尾斜杠或路径。应用默认只发 Secure、HttpOnly、SameSite=Lax、Path=/ 的 `__Host-favkeep_session` Cookie；反向代理保留浏览器 Origin，不需要信任任意 Forwarded/X-Forwarded-* 头。

开发时可显式使用 `--insecure-cookies --app-origin http://127.0.0.1:24188`，此时 Cookie 名为 `favkeep_session_dev`。HTTP Origin 只允许 localhost 或回环地址；公网必须使用 HTTPS。Vite 开发入口使用对应的 `http://localhost:5173` Origin。

未设置管理员时，启动会在 `--config-dir` 指定的目录生成 `auth-setup.secret`。日志只打印文件位置；Linux 文件权限为 0600，Windows 限制到当前用户。服务器维护者读取该文件后，在 `/setup` 输入初始化码并设置用户名、密码。不要把文件加入版本库或发给其他人。成功后初始化入口关闭并删除文件，重启不会重新开放。重复或并发初始化只会成功一次。

密码为 15–128 个字符，不裁剪密码空白。采用 Argon2id（19 MiB、2 次迭代、并行度 1），每个密码单独使用随机盐；哈希最多同时执行两个。登录及初始化失败有渐进限速，返回 429 和 Retry-After 时稍后重试，不设置永久锁定。

## 会话与设置

会话存入现有 SQLite 数据库，仅保存会话标识的 SHA-256 哈希，不保存原始 Cookie。闲置 24 小时或登录后 7 天到期；服务正常重启保留未到期会话。退出撤销当前会话，改密撤销其他会话并轮换当前会话和 CSRF token。相关 WebSocket 在服务端关闭，媒体的新请求被拒绝；已经下载或缓冲的数据无法撤回。

所有写请求必须提供匹配的 Origin；已登录的写请求还需通过 `X-CSRF-Token`。登录状态及 CSRF token 由 `GET /api/auth/session` 返回，浏览器仅在内存中保存。WebSocket 也校验 Origin。

普通 `GET/PUT /api/config` 不再返回或接受旧应用 token、B 站凭据、通知器秘密。凭据与通知器通过独立接口保存，配置版本冲突返回 409。编辑通知器时省略秘密字段表示保留，显式提交空值表示清除允许为空的秘密；Webhook Header 替换为整组操作。删除通知器前界面要求确认。B 站扫码成功由服务端直接保存，前端只收到成功状态；旧的 GET 轮询入口停止使用。

旧 Authorization token、localStorage token、WebSocket 子协议 token 和 `/api/media-session` 均停止认证。旧数据库中的 auth_token 仅为配置读取兼容保留，没有访问权限。外部脚本必须改为登录建立会话，并按上述规则发送 Origin/CSRF；不提供永久 API token。

平台凭据保存在服务器配置目录的数据库及账号文件中，接口脱敏不等于应用层加密；应限制配置目录及备份文件权限。

## 本机密码恢复与备份

忘记密码时先停止应用，再在服务器上运行：

`favkeep --config-dir /path/to/config --reset-password`

命令以隐藏输入两次读取新密码，保留用户名、媒体及观看记录，并撤销全部会话。随后按原配置重新启动。不要通过命令行参数或环境变量传递密码。

升级前停止实例或使用 SQLite 在线 backup 创建一致性副本，备份程序与数据库为一组。恢复备份后，首次启动前清空 `app_session` 并提升 `app_identity.auth_version`，确保旧备份不会恢复已撤销的登录。管理员尚未初始化时，恢复后仍须使用服务器初始化码，不能让外部访客认领。

## 自检

后端：`cargo test -p favkeep --locked`。既有真实账号及人工扫码测试保持跳过。

前端：`bun run check`、`bun run lint`、`bun run build`。独立预览使用 `bun run preview -- --host 127.0.0.1 --port 5183 --strictPort`。

浏览器脚本依赖测试环境中已有的 Playwright 与 Edge，不新增应用依赖：

- `node scripts/check_auth.cjs`：模拟 API 验证初始化、登录、刷新、退出、401、CSRF 请求头、旧 token 清理、安全返回地址及布局。
- 设置 `FAVKEEP_TEST_ORIGIN=http://127.0.0.1:5183` 后运行 `node scripts/check_watch.cjs <本地 MP4 样本>`：验证新版观看区。样本至少 5 秒且 Edge 可播放。

截图写入忽略的 `local-data/auth-smoke` 和 `local-data/watch-smoke`。模拟浏览器测试不能代替树莓派、真实平台账号或公网验收。
