# favkeep 快速开始

当前提供源码构建方式。上游 bili-sync 的发布包和 Docker 镜像不包含 favkeep 功能，请从本仓库构建。升级现有实例前先阅读[升级与回退](operations/deployment.md)，不要用测试数据覆盖旧数据库。

## 依赖

- Git，以及 `rust-toolchain.toml` 指定的 Rust（建议通过 rustup 安装）。
- Bun 1.4.4（工作流固定版本），用于 Web 和文档依赖；使用仓库锁文件。
- FFmpeg 与 ffprobe，均须位于运行程序的 PATH 中。
- 对应平台的 Rust 原生构建工具：Linux 的 C/C++ 工具链、pkg-config 和 Perl，Windows GNU 的 MinGW-w64 工具链，或相应的 MSVC 构建工具。按锁定依赖构建，首次构建需要联网下载依赖。

Rust 程序内嵌前端产物，必须先构建 Web。终端进入希望存放源码的目录后执行：

```sh
git clone https://github.com/YangChen-mechanical/favkeep.git
cd favkeep
cd web
bun install --frozen-lockfile
bun run build
cd ..
cargo build --release --locked
```

Windows GNU 若在链接时报缺少 `libgcc_eh`，可在当前 PowerShell 设置 `$env:RUSTFLAGS = '-C linker=rust-lld.exe -C link-self-contained=yes'` 后重新构建。

可执行文件位于 `target/release/favkeep`，Windows 为 `target/release/favkeep.exe`。工程与程序统一使用 favkeep；系统默认配置位置下的新目录为 `favkeep`。旧实例须先按[一次性复制迁移](operations/deployment.md#一次性复制到新目录)复制运行数据，不会自动沿用旧目录。

## 首次本机启动

新建独立的配置与媒体目录，使用绝对路径。以下示例在仓库被忽略的 `local-data/` 下保存运行数据；迁移或删除源码前应另行备份。

Linux / macOS：

```sh
mkdir -p local-data/config local-data/media
./target/release/favkeep \
  --config-dir "$(pwd)/local-data/config" \
  --media-root "$(pwd)/local-data/media" \
  --app-origin http://127.0.0.1:12345 \
  --insecure-cookies
```

Windows PowerShell：

```powershell
New-Item -ItemType Directory -Force local-data/config, local-data/media | Out-Null
$configDir = (Resolve-Path local-data/config).Path
$mediaDir = (Resolve-Path local-data/media).Path
./target/release/favkeep.exe --config-dir $configDir --media-root $mediaDir --app-origin http://127.0.0.1:12345 --insecure-cookies
```

程序默认监听 `0.0.0.0:12345`；本机示例必须配合防火墙限制外部访问。`--insecure-cookies` 仅用于 localhost 或回环地址的 HTTP，不能用于公网。网页使用的地址必须与 `--app-origin` 完全一致，包括主机名、协议和端口。

1. 服务启动后，从本机 `local-data/config/auth-setup.secret` 读取一次性初始化码，勿上传或分享。
2. 打开 `http://127.0.0.1:12345/setup`，输入初始化码，创建管理员。密码长度为 15–128 个字符。
3. 在管理页连接自己的平台账号，设置来源及可写媒体路径，再启用同步。B 站来源的路径也要显式设置到你的媒体目录；`--media-root` 只提供抖音、小红书的默认路径。
4. 打开 `/watch` 查看已归档且可播放的本地视频。首次同步不一定下载历史，请先核对各平台的基线规则。

关闭服务可在前台终端按 Ctrl+C，等待任务和数据库正常关闭。

## 服务器与升级

服务器部署请通过防火墙或容器端口映射限制应用监听地址。公网入口必须使用 HTTPS，例如 `--app-origin https://favkeep.example.com`，并去掉 `--insecure-cookies`；反向代理需保留正确的 Origin，并支持媒体 Range 与 WebSocket。当前版本尚未完成公网部署安全验收，建议先通过受控私有网络使用。

配置目录持久化保存数据库、管理员身份、会话及平台凭据。升级前保存匹配版本的程序、配置与 SQLite 一致性备份，回退必须成组恢复；不能仅替换旧程序后继续使用新库。

[认证与密码恢复](operations/authentication.md) · [媒体目录与 Docker 构建示例](operations/media-paths.md) · [升级与回退](operations/deployment.md) · [开发检查](development/README.md)
