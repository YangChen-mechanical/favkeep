# Third-party notices

favkeep is derived from [amtoaer/bili-sync](https://github.com/amtoaer/bili-sync), under the MIT License. Its copyright and permission notice are preserved in [License](License). The main project license does not replace the licenses of the components below.

## Source included or adapted

| Component | Source and version | License and retained notice | Changes in favkeep |
| --- | --- | --- | --- |
| bili-sync | Upstream Git history retained in this repository | MIT; [License](License) | Administrator authentication, watching UI, progress, and additional archive platforms |
| danmu2ass | [gwy15/danmu2ass](https://github.com/gwy15/danmu2ass); inherited through bili-sync | MIT; [licenses/danmu2ass-MIT.txt](licenses/danmu2ass-MIT.txt) | Rust danmaku layout and ASS output in `crates/bili_sync/src/bilibili/danmaku/`, inherited and adapted through upstream |
| F2 | [Johnserf-Seed/f2, f6be8c0ffba9a127075bbeafe4838716650b6325](https://github.com/Johnserf-Seed/f2/tree/f6be8c0ffba9a127075bbeafe4838716650b6325), `f2/utils/crypto/bytedance/abogus.py`; author JohnserfSeed | Apache-2.0; [LICENSE-F2](crates/bili_sync/src/douyin/LICENSE-F2) | Ported to Rust in `examples/browserless/legacy_sign.rs`; deterministic offline fixtures and generation script. This legacy signer is a development reference, not the current production signer. No upstream NOTICE file was present in the checked reference. |
| catbus | [cv-cat/catbus, bcc72f81549b94090b340fb87f29927529056e49](https://github.com/cv-cat/catbus/tree/bcc72f81549b94090b340fb87f29927529056e49) | MIT; [catbus-LICENSE](crates/bili_sync/src/douyin/native/catbus-LICENSE) | Rust Passport field/signature encodings and device-profile references in `src/douyin/native/` and the browserless example |
| xhshow | [Cloxl/xhshow, b242a84f9c5a6a971bc4c90c63ad43345a4bf8a1](https://github.com/Cloxl/xhshow/tree/b242a84f9c5a6a971bc4c90c63ad43345a4bf8a1) | MIT; [xhshow-LICENSE](crates/bili_sync/src/xiaohongshu/xhshow-LICENSE) | Selected signature logic ported to Rust in `src/xiaohongshu/sign.rs`; synthetic offline fixtures |

Paths in the table are relative to `crates/bili_sync/` where abbreviated. Existing source headers retain the component-specific SPDX identifiers and attribution. Upstream platform documentation is retained as reference and may describe older behavior.

## Dependencies and distribution

Cargo and Bun dependencies remain under their own licenses. Their exact versions and sources are recorded in `Cargo.lock`, `web/bun.lock`, and `docs/bun.lockb`. The entries above cover directly adapted source, not a complete license inventory of transitive packages.

Before distributing binaries or container images, collect the license and notice texts for the dependencies actually linked or shipped, including FFmpeg and the selected base image. The current public delivery is source code; release and documentation workflows only build reviewable artifacts and do not publish packages, images, or a website automatically.

The code license does not grant rights to third-party videos, music, cover art, platform names, or trademarks. Upstream media screenshots have been omitted from this source release; the application favicon is a generic favkeep mark.
