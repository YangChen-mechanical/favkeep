# 三平台媒体目录

以下使用 `/srv/favkeep` 作为示例宿主机目录，请替换为自己的绝对路径：

```text
/srv/favkeep/
├── config/                        # 数据库、管理员、会话与平台凭据
└── media/
    ├── bilibili/                  # 路径由 B 站来源配置指定
    ├── douyin/<作品ID>/
    │   ├── video.mp4
    │   └── poster.jpg
    └── xiaohongshu/<作品ID>/
        ├── video.mp4
        └── poster.jpg
```

`poster.jpg` 为尽力生成，失败不影响视频播放。账号凭据与数据库不写入媒体根目录。运行时需要 ffmpeg 和 ffprobe，无需 Chromium。

## 原生程序

```sh
./favkeep --config-dir /srv/favkeep/config --media-root /srv/favkeep/media --app-origin http://127.0.0.1:12345 --insecure-cookies
```

`--media-root` 或 `FAVKEEP_MEDIA_ROOT` 只提供抖音、小红书的默认路径。B 站来源路径需要在管理页配置。已有 `douyin.path`、`xiaohongshu.path` 优先，更换默认根目录不会搬动旧文件或改写数据库路径。

## 自行构建 Docker 镜像

仓库 `Dockerfile` 使用事先构建的 Linux musl 压缩包；不能直接复制本机 Windows 或 GNU 二进制到 Alpine 中运行。具备相应交叉编译工具的 Linux 开发环境可使用 `just build-docker` 构建本地 `favkeep-local` 镜像。容器流程与各目标架构须另行验证；首次使用建议按[快速开始](../quick-start.md)原生运行。

自行验证镜像后，可使用以下 Compose 挂载。先创建宿主机目录，并确保服务运行用户可写；示例只将端口发布到宿主机回环地址。

```yaml
services:
  favkeep:
    image: favkeep-local
    user: "1000:1000"
    restart: unless-stopped
    ports:
      - "127.0.0.1:12345:12345"
    environment:
      FAVKEEP_APP_ORIGIN: http://127.0.0.1:12345
      FAVKEEP_INSECURE_COOKIES: "true"
      FAVKEEP_MEDIA_ROOT: /media
    volumes:
      - /srv/favkeep/config:/app/.config/favkeep
      - /srv/favkeep/media/bilibili:/downloads
      - /srv/favkeep/media/douyin:/media/douyin
      - /srv/favkeep/media/xiaohongshu:/media/xiaohongshu
```

此例的 B 站来源路径应使用 `/downloads` 及其子目录。浏览器须位于服务器本机，或使用保留同一回环 Origin 的私有转发。HTTPS 反向代理部署须设置实际 HTTPS Origin 并移除 insecure Cookie 选项。

## 迁移已有实例

先按[一次性复制迁移](deployment.md#一次性复制到新目录)将数据库与账号文件复制到新配置目录。旧视频保留原位置；若视频位于旧配置目录内，容器仍须将它挂载到原容器路径。只把数据库挂载改到新目录会导致旧媒体不可访问。

保留旧 B 站宿主机与容器路径映射，不能把原 `/downloads` 改挂到三平台共同父目录。新增两平台目录即可；不要移动旧文件或递归修改旧目录权限。

需要修改已保存的平台路径时，先停用来源、等待任务结束并备份，再通过管理页或受保护配置接口按配置版本更新 `path`，保留其他字段。旧成品仍通过数据库中的原路径播放，不会自动迁移。
