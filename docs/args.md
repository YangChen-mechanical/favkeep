# 命令行参数

程序支持有限的命令行参数，可以通过执行 `bili-sync-rs --help` 查看说明。

```shell
bili-sync/target/debug main* ⇡
❯ ./bili-sync-rs --help
由 Rust & Tokio 驱动的哔哩哔哩同步工具

Usage: bili-sync-rs [OPTIONS]

Options:
  -s, --scan-only              [env: SCAN_ONLY=]
  -l, --log-level <LOG_LEVEL>  [env: RUST_LOG=] [default: None,bili_sync=info]
  -h, --help                   Print help
  -V, --version                Print version
```

上面是早期版本的帮助示例，完整参数以当前程序的 `--help` 输出为准。参数除可以通过命令行设置外，还可通过环境变量设置。

## `--media-root`

可选参数，对应环境变量 `BILI_SYNC_MEDIA_ROOT`，指定抖音与小红书新配置的默认媒体父目录，须使用绝对路径。例如 `--media-root /media` 分别生成 `douyin.path=/media/douyin` 与 `xiaohongshu.path=/media/xiaohongshu`。

只在对应平台的配置或 `path` 字段缺省时生效；已有配置路径优先，不移动文件、不修改数据库中已有媒体路径，也不影响 B 站来源路径、命名模板或 `/downloads` 挂载。未设置时沿用原默认值：配置目录下的 `douyin-media` 与 `xiaohongshu-media`。

宿主机目录与 Docker 挂载示例见 [三平台媒体目录](operations/media-paths.md)。

## `--scan-only`

`--scan-only` 参数用于仅扫描列表，而不实际执行下载操作。该参数的主要目的是[方便用户从 v1 迁移](https://github.com/amtoaer/bili-sync/issues/66#issuecomment-2066642481)，新用户不需要关注。

## `--log-level`

`--log-level` 参数用于设置日志级别，一般可以维持默认。该参数与 Rust 程序中 `RUST_LOG` 的语义相同，可以查看[相关文档](https://docs.rs/env_logger/latest/env_logger/#enabling-logging)获取详细信息。
