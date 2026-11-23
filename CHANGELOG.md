# Changelog

包版本（`luci-app-natfrp`）与后端版本（`natfrpctl`）独立演进。
格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)。

## [Unreleased]

### Changed

- CI 获取守护进程二进制改为「CDN 优先、仓库存档兜底」：`vendor/blobs/` 内置解包后的
  `natfrp-service` / `natfrp-frpc`，CDN 拉取或解包失败时自动回退，并始终按
  `scripts/vendor-sha256.txt` 校验；存档损坏或与钉死值不符会直接失败。
- CI 增加 Cargo 依赖与构建缓存、同一 PR 的旧任务自动取消、任务超时保护，
  CDN 与 OpenWrt SDK 下载增加重试。

## [3.2.1] — 2026-11-24

### Added

- `natfrpctl` 1.1.0：新增 `set-token`、`restore` 子命令。
- WebUI 证书改由 `natfrpctl` 生成（带 SAN、825 天），路径写入 `config.json`。
- 配置快照 `/etc/natfrp-config.snapshot`（在 `/etc/natfrp` 之外），
  `postinst` 在配置丢失时自动 `restore`。
- `natfrpctl apply --restart`：只改 token 时也能强制重载。
- `natfrpctl status` 增加远程认证方式字段。

### Changed

- 访问密钥不再写入 uci，改由 `set-token` 直接写 `config.json`（0600）。
- 远程管理、远程改配置、远程触发更新默认关闭。
- relay：15 秒读写超时、32 连接上限（超出回 503）、请求 body 泵送、
  `openssl` 可执行位检查、WebSocket 退出时唤醒转发线程。
- init 脚本配置非法时不再返回失败（避免 procd 重启风暴）；启动时清 LuCI 缓存。
- 随机口令改用拒绝采样，消除模偏差。
- CI 增加 `cargo fmt --check`，预编译 Rust 组件并校验格式。

### Fixed

- 导入自签证书后浏览器仍报名称不匹配、iOS/macOS 直接拒绝（证书无 SAN、有效期 10 年）。
- 只修改访问密钥时服务不重载。
- IPv6 地址访问时「打开 WebUI」链接非法。
- `secrets` 失败时统一误报「需要写权限」，现区分配置损坏。
- `PKG_LICENSE` 只标注 MIT，未体现分发的 AGPL 二进制。
- relay 对大请求体只转发首块，可能截断。

## [3.2.0] — 2026-11-23

### Added

- 首个 Rust 后端 + 原生 LuCI JS 版本，移除对 Lua / `luci-compat` 的依赖。
- 单包 APK：守护进程、`frpc`、`natfrpctl`、页面、init 脚本。
- WebUI 默认 TLS；可关闭 TLS 改用明文 relay（含 WebCrypto 兼容层）。
- WebUI/远程管理口令自动生成；配置损坏保护与 `.bak` 恢复；
  只读账号专用 `natfrpctl-status` 入口。
- CI：Node 自测、Rust 交叉编译、官方二进制 sha256 校验、SDK 打包与 Releases 发布。

## [3.1.9] — 2026-11（官方基线）

- 守护进程 `natfrp-service 3.1.9`、`frpc 0.51.0-sakura-15.1`，取自官方 CDN。
