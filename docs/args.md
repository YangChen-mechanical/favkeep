# 命令行参数

通过 `favkeep --help` 查看当前完整帮助，`favkeep --version` 查看版本。Windows 程序为 `favkeep.exe`。

| 参数 | 环境变量 | 用途 |
| --- | --- | --- |
| `--config-dir` / `-c` | `FAVKEEP_CONFIG_DIR` | 数据库与账号运行目录，默认系统配置位置下的 `favkeep` |
| `--media-root` | `FAVKEEP_MEDIA_ROOT` | 新配置的抖音、小红书媒体父目录，须为绝对路径 |
| `--ffmpeg-path` / `-f` | `FAVKEEP_FFMPEG_PATH` | FFmpeg 程序位置，默认 `ffmpeg`；ffprobe 从同目录寻找 |
| `--app-origin` | `FAVKEEP_APP_ORIGIN` | 网页完整 Origin，包含协议、主机和端口 |
| `--insecure-cookies` | `FAVKEEP_INSECURE_COOKIES` | 仅用于本机 HTTP 开发 |
| `--scan-only` / `-s` | `SCAN_ONLY` | 仅扫描，不实际下载 |
| `--log-level` / `-l` | `RUST_LOG` | 日志筛选，默认 `None,favkeep=info` |
| `--disable-credential-refresh` / `-d` | `DISABLE_CREDENTIAL_REFRESH` | 停用 B 站凭据自动刷新 |
| `--reset-password` | — | 停止服务后重置管理员密码 |
| `--douyin-import-session` | — | 停止服务后导入同账号私有会话 |
| `--migrate-config-from` | — | 停止旧服务后复制运行数据，须同时指定目标 `--config-dir` |

配置目录、媒体根目录和 FFmpeg 路径保留对应的 `BILI_SYNC_*` 环境变量别名。优先级为命令行 → 新环境变量 → 旧环境变量 → 默认值；无效值报错，不静默退回旧值。迁移命令不能与密码重置或会话导入一起执行。迁移步骤见[升级与回退](operations/deployment.md#一次性复制到新目录)。

## `--media-root`

可选参数，对应环境变量 `FAVKEEP_MEDIA_ROOT`，指定抖音与小红书新配置的默认媒体父目录，须使用绝对路径。例如 `--media-root /media` 分别生成 `douyin.path=/media/douyin` 与 `xiaohongshu.path=/media/xiaohongshu`。

只在对应平台的配置或 `path` 字段缺省时生效；已有配置路径优先，不移动文件、不修改数据库中已有媒体路径，也不影响 B 站来源路径、命名模板或 `/downloads` 挂载。未设置时沿用原默认值：配置目录下的 `douyin-media` 与 `xiaohongshu-media`。

宿主机目录与 Docker 挂载示例见 [三平台媒体目录](operations/media-paths.md)。

## `--scan-only`

`--scan-only` 参数用于仅扫描列表，而不实际执行下载操作。该参数的主要目的是[方便用户从 v1 迁移](https://github.com/amtoaer/bili-sync/issues/66#issuecomment-2066642481)，新用户不需要关注。

## `--log-level`

`--log-level` 参数用于设置日志级别，一般可以维持默认。该参数与 Rust 程序中 `RUST_LOG` 的语义相同，可以查看[相关文档](https://docs.rs/env_logger/latest/env_logger/#enabling-logging)获取详细信息。
