# 代码结构与依赖方向

本文描述当前源码的模块职责与依赖方向。

favkeep 是模块化单体：一个 Rust 服务、内嵌的 Svelte 静态前端、一份 SQLite 数据库，以及独立的媒体和凭据目录。三个平台使用各自的同步规则，共用应用认证、媒体访问和观看进度。

```text
crates/
├── favkeep/src/
│   ├── main.rs                  启动、关闭和任务生命周期
│   ├── migrate_config.rs        离线复制运行数据、校验及新目录发布
│   ├── api/routes/              HTTP 入口、参数与响应
│   │   └── videos/              query 查询、mutations 状态修改、watch 汇总
│   ├── auth/                    state 认证状态、session 会话、password 密码、middleware 防护、routes 入口
│   ├── bilibili/                B 站协议、来源和下载流程
│   ├── douyin/                  抖音原生协议、登录、编排、存储及进度
│   ├── xiaohongshu/             小红书原生协议、登录、编排、存储及进度
│   ├── platform/               平台标识、能力和公共错误／ID 校验
│   ├── config/                 配置校验、版本及持久化
│   ├── task/                   HTTP 服务与三平台调度
│   ├── media.rs                媒体状态与可播放性
│   ├── media_file.rs           成品校验、空间检查和中断恢复标记
│   ├── downloader.rs           公共下载与资源限制
│   └── utils/                  Cookie、私有文件及其它明确用途的工具
├── favkeep_entity/           SeaORM 实体与自定义字段类型
└── favkeep_migration/        按历史顺序执行的数据库迁移
web/src/
├── routes/                     URL 入口和页面组合
└── lib/
    ├── api.ts                  稳定的统一调用入口
    ├── api/                    http 请求；按认证、媒体、观看、来源、配置、平台、系统分组
    ├── types.ts                稳定的统一类型入口
    ├── types/                  按相同业务分类的传输类型
    ├── platforms/              平台任务请求、轮询与状态生命周期
    ├── components/platforms/   平台面板及共用任务展示
    ├── components/ui/          既有基础 UI
    └── stores/                 登录、导航及筛选等共享状态
```

## 调用方向

```text
页面 → 业务面板／状态 → API 方法 → 统一 HTTP 请求 → Rust 路由
Rust 路由／调度 → 平台登录与运行模块 → 平台客户端／存储模块
平台客户端 → 公共 Cookie 与私有文件工具
存储模块 → 实体／SQLite；数据库启动 → 历史迁移
播放器 → 受会话保护的媒体接口／观看进度接口
离线迁移命令 → SQLite 一致性快照／账号文件 → 校验 → 新配置目录
```

- 三个平台不通过另一个平台获得公共类型。公共错误与 ID 校验在 `platform`，平台签名、收藏分页和基线规则留在各平台。
- 抖音按收藏夹维护基线，小红书维护前 50 个不同条目的窗口。相似代码不代表业务规则相同，不合成一个可配置同步引擎。
- `api/http.ts` 是前端唯一的 HTTP 发送实现，统一凭据、CSRF、错误和失效会话处理。统一入口直接组合业务方法，不再逐项包装转发。
- 共用任务控制器负责请求、间隔校验、忙碌状态、轮询和卸载；面板负责展示与提示，平台入口明确提供各自调用、文案和统计说明。
- 生产媒体工具和探针的私有输出工具分别保留：前者允许恢复已有目录及原子替换凭据，后者要求全新目录和排他创建，不能为了去重降低隔离要求。

## 测试与工具

认证测试位于 `auth/tests.rs`，视频接口测试位于 `api/routes/videos/tests.rs`，平台持久化测试位于各自 `store/tests.rs`；小型纯函数测试保留在实现旁。前端请求和任务状态测试位于对应业务目录。

`examples/protocol_probe.rs` 用于隔离验证平台登录、会话恢复、收藏读取与媒体下载，复用生产客户端，不打开业务数据库。各阶段实现位于 `examples/protocol/`，验证输出与应用运行数据分开保存。

本地运行数据、凭据、数据库、截图和下载样本在忽略的 `local-data/` 或应用配置目录；`target/`、`web/build/`、`.svelte-kit/` 和 `node_modules/` 是构建输出或缓存，不进入业务源码。

运行命令和维护约定见[开发说明](development/README.md)。
