# 开发与检查

工程使用工具链文件指定的 Rust，以及 Bun 管理 Web 依赖。Web 与文档项目各自保留锁文件；不要再生成 `web/package-lock.json`。

## Web

在 `web/` 下执行：

```sh
bun install --frozen-lockfile
bun run test
bun run check
bun run lint
bun run build
```

`test` 使用 Bun 运行已有 `node:test` 用例，不需要额外测试框架。请求测试使用模拟 fetch；任务状态测试使用内存状态，不连接服务器或平台账号。开发服务器用 `bun run dev`。

## Rust

先构建 Web，再在仓库根目录执行：

```sh
cargo fmt --all --check
cargo test --workspace --locked --offline
cargo test --locked --offline -p favkeep --example protocol_probe
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo build --workspace --locked --offline
```

`--offline` 需要本机已有依赖缓存；首次准备缓存时可去掉。真实扫码／网络测试维持显式跳过，不把它们当作离线通过。

Windows GNU 环境若系统 GCC 缺少 `libgcc_eh`，可仅为当前终端设置 Rust 自带链接器，不修改项目的目标配置：

```powershell
$env:RUSTFLAGS = '-C linker=rust-lld.exe -C link-self-contained=yes'
```

## 文档

在 `docs/` 下执行 `bun install --frozen-lockfile` 和 `bun run docs:build`。内部文档通过 Git 忽略和 VitePress 排除规则隔离；保留本地内部文件时，构建产物和搜索索引也不得包含其正文。

## 前端自检与协议验证

- `scripts/check_auth.cjs`：使用模拟后端检查认证页面。
- `scripts/check_watch.cjs <MP4样本>`：观看、进度和请求竞争的浏览器自检。
- `scripts/check_douyin.cjs <MP4样本>`：三平台页面、任务和配置自检。
- 上述脚本通过 `FAVKEEP_TEST_ORIGIN` 指向本地生产预览，使用 Playwright 的 Edge 浏览器。测试工具由开发环境提供，不是应用运行依赖。
- `scripts/generate_douyin_sign_vectors.py`：离线重建签名对照数据，使用前核对固定参考版本。
- 原生探针的离线测试可直接运行；带 `--live` 的命令会访问真实平台，需单独安排，不属于普通本地检查。

## 修改约定

先搜索调用方，再调整模块；业务实现与对应测试一起迁移。共享实现须有真实的共同调用方；平台差异留在各自模块。数据库迁移、对外配置和媒体路径不因目录重构改名。检查失败须区分既有问题与本次引入的问题。

[结构说明](../architecture.md) · [开发路线](roadmap.md)
