# SakuraFrp for OpenWrt · luci-app-natfrp

[![build](https://github.com/LianXia233/natfrp-openwrt/actions/workflows/build-apk.yml/badge.svg?branch=main)](https://github.com/LianXia233/natfrp-openwrt/actions/workflows/build-apk.yml)
[![release](https://img.shields.io/badge/release-latest%20APK-blue)](https://github.com/LianXia233/natfrp-openwrt/releases/latest)
[![package](https://img.shields.io/badge/package-3.2.1--r1-blue)](https://github.com/LianXia233/natfrp-openwrt/releases/latest)
[![platform](https://img.shields.io/badge/OpenWrt-25.12%2B-00b5e2)](https://openwrt.org)
[![arch](https://img.shields.io/badge/arch-aarch64__cortex--a53-informational)](#兼容性)
[![license](https://img.shields.io/badge/license-MIT%20%2B%20AGPL--3.0-green)](LICENSE)

SakuraFrp 启动器在 OpenWrt 上的集成：**Rust 后端 `natfrpctl` + 原生 LuCI JS 页面**，不使用 Lua、
不依赖 `luci-compat`。守护进程与 `frpc` 直接取官方 3.1.9 预编译件，本仓库负责配置转换、
页面、服务管理和打包。

已在 **ImmortalWRT SNAPSHOT（mediatek/filogic，`aarch64_cortex-a53`）** 上实机验证：
安装、升级、TLS/relay 两种入口、隧道启动、远程管理连接均正常。

---

## 目录

- [版本与兼容性](#版本与兼容性)
- [特性](#特性)
- [安装](#安装)
- [首次使用](#首次使用)
- [配置说明](#配置说明)
- [运行时行为](#运行时行为)
- [升级、迁移与卸载](#升级迁移与卸载)
- [构建](#构建)
- [排障](#排障)
- [已知限制](#已知限制)
- [安全说明](#安全说明)
- [目录结构](#目录结构)
- [更新日志](#更新日志)
- [License](#license)

---

## 版本与兼容性

### 组件版本

| 组件 | 版本 | 来源 |
|---|---|---|
| `luci-app-natfrp`（APK） | **3.2.1-r1** | 本仓库 CI 构建 |
| `natfrpctl`（Rust 后端） | **1.1.0** | 本仓库源码 |
| `natfrp-service`（守护进程） | **3.1.9** | 官方 CDN，按 sha256 钉死 |
| `natfrp-frpc` | **0.51.0-sakura-15.1** | 同上 |
| LuCI 页面 | 原生 JS | 本仓库 |

### 兼容性

| 项目 | 要求 |
|---|---|
| 包管理器 | **apk**（OpenWrt 25.12+ / ImmortalWRT SNAPSHOT） |
| 发行版 | 当前仅发布 **`aarch64_cortex-a53`** 单包（target `mediatek/filogic`） |
| 依赖 | `luci-base`、`openssl-util`（证书生成与明文 relay 用） |
| 存储 | 安装后约 **36 MiB**（两个静态二进制占绝大部分） |

> `natfrpctl` 的交叉编译目标在 CI 里固定为 `aarch64-unknown-linux-musl`。
> 其他架构需要同时调整 `RUST_TARGET` 与 `files/blobs/<arch>`，当前不提供成品包。

---

## 特性

### 集成与体验

- **原生菜单/ACL**：`menu.d` + `rpcd/acl.d`，与 LuCI JS 前端一致，不加载 Lua 兼容层。
- **单包安装**：守护进程、`frpc`、`natfrpctl`、页面、init 脚本在一个 APK 里。
- **一键入口**：远程管理配置好后，页面主按钮直达 `https://www.natfrp.com/remote/v2`；
  否则打开本机 WebUI。
- **口令自动生成**：WebUI 连接密码、远程管理 E2E 密码首次启用时从 `/dev/urandom`
  生成 20 位（拒绝采样），不存在默认口令。

### 安全

- **访问密钥不经 uci**：保存时由 `natfrpctl set-token` 直接写 `/etc/natfrp/config.json`（0600）。
  旧做法放在 `/etc/config/natfrp`，只读 LuCI 账号有 uci 读权限，能在“保存 → 应用”之间读到明文。
- **默认收紧**：远程管理默认关闭，「允许远程改配置 / 触发更新」默认关闭。上游 E2E 密钥是
  固定盐 + 1 万次 PBKDF2，泄露密码的代价必须可控。
- **证书可用**：`natfrpctl` 生成带 **SAN**、**825 天** 的证书并写进配置。
  守护进程自签的证书没有 SAN 且有效期 10 年——Chrome 导入后仍报名称不匹配，iOS/macOS 直接拒绝。
- **配置不会静默丢失**：解析失败先试 `.bak`，再不行就放弃写入并报错；每次成功写入镜像一份
  `/etc/natfrp-config.snapshot`（在 `/etc/natfrp` 之外），升级/误删后可恢复。

### 运维

- **明文入口隔离**：关闭 TLS 时守护进程只监听回环，`natfrpctl relay` 对外提供 HTTP，
  并注入 WebCrypto 兼容层；relay 有 15 秒读写超时与 32 连接上限。
- **可观测**：`natfrpctl status` 输出运行状态、版本、端口与敏感项是否已设置（不含口令）；
  只读账号通过 `natfrpctl-status` 白名单执行。

---

## 安装

从 [Releases](https://github.com/LianXia233/natfrp-openwrt/releases/latest) 下载最新
`luci-app-natfrp-*.apk`，然后：

```sh
apk add --allow-untrusted ./luci-app-natfrp-3.2.1-r1.apk
```

- 包未做 apk 签名（本地/CI 构建），`--allow-untrusted` 是必需的。
- `postinst` 会先尝试从 `/etc/natfrp-config.snapshot` 恢复配置，再清 LuCI 缓存并重启 `rpcd`；
  若检测到之前已启用服务（`/etc/rc.d/S90natfrp` 存在）会顺手拉起。
- 版本核对：

```sh
apk info -v | grep natfrp
natfrpctl version          # {"ok":true,"version":"1.1.0"}
```

---

## 首次使用

打开 **服务 → SakuraFrp 内网穿透**：

1. 勾「启用服务」，填访问密钥（服务端控制台获取），保存并应用。
2. WebUI 默认开启（`0.0.0.0:4101`，HTTPS）；远程管理默认关闭，需要时再勾上，
   E2E 密码留空会自动生成。
3. 页面顶部显示连接密码与远程管理密码，各带复制按钮；证书按钮只在 TLS 开启时出现。
4. 自动启动隧道：填隧道 ID（逗号分隔），或用页面上的链接到 natfrp.com 管理。

---

## 配置说明

uci 配置文件：`/etc/config/natfrp`（包内为 conffile，升级保留）。

| 选项 | 默认 | 说明 |
|---|---|---|
| `enabled` | `0` | 开机自启；保存后立即启动/停止 |
| `token` | — | 访问密钥。**不落 uci**，保存时写入 `config.json` |
| `webui` | `1` | 启用启动器自带 WebUI |
| `webui_tls` | `1` | `1`：直接 HTTPS；`0`：回环 HTTPS + 明文 relay |
| `webui_host` | `0.0.0.0` | 监听地址（relay 模式下为明文入口地址） |
| `webui_port` | `4101` | 监听端口；关闭 TLS 时必须 **> 1000** |
| `webui_pass` | 自动 | 连接密码；留空则首次生成，之后留空表示不修改 |
| `remote_mgmt` | `0` | 启用远程管理（出站 WSS） |
| `remote_mgmt_pass` | 自动 | E2E 密码；留空首次生成，KDF 后只存派生值 |
| `remote_allow_config` | `0` | 允许远端改配置（默认关） |
| `remote_allow_update` | `0` | 允许远端触发更新（默认关） |
| `auto_start_tunnels` | — | 自动启动的隧道 ID，逗号/空格分隔 |
| `check_update` | `0` | 每周检查启动器更新（写 flash，按需开启） |

### 后端命令

| 命令 | 用途 | 执行权限 |
|---|---|---|
| `natfrpctl status` | 运行状态（不含口令） | 只读（经 `natfrpctl-status`） |
| `natfrpctl version` | 版本号 | 只读 |
| `natfrpctl secrets` | WebUI / 远程管理口令 | 写权限 |
| `natfrpctl set-token <密钥>` | 写访问密钥（`--clear` 清除） | 写权限 |
| `natfrpctl apply [--restart]` | 同步配置并重启服务 | 写权限 |
| `natfrpctl render` | 只生成 `config.json`（开机调用） | root |
| `natfrpctl restore` | 配置丢失时从快照恢复 | root |
| `natfrpctl relay` | 明文 HTTP 入口（TLS 关闭时） | root |
| `natfrpctl webui-cert` | 输出当前 WebUI 证书 PEM | 写权限 |

---

## 运行时行为

### 端口

| `webui_tls` | 守护进程 | 对外入口 |
|---|---|---|
| `1` | `webui_host:webui_port`（HTTPS，默认 `0.0.0.0:4101`） | 同上 |
| `0` | `127.0.0.1:(webui_port+1000)`（HTTPS，如 `5101`） | `natfrpctl relay` 监听 `webui_host:webui_port`（HTTP） |

```sh
netstat -ltnp | grep -E ':(4101|5101) '
curl -sk https://127.0.0.1:5101/ -o /dev/null -w '%{http_code}\n'   # TLS 开启或 relay 模式下均有
curl -s  http://192.168.88.1:4101/ -o /dev/null -w '%{http_code}\n' # relay 模式
```

### 文件与权限

| 路径 | 权限 | 内容 |
|---|---|---|
| `/etc/config/natfrp` | 0600 | uci 配置（不含 token/口令明文） |
| `/etc/natfrp/config.json` | 0600 | 守护进程配置（含 token、WebUI 密码） |
| `/etc/natfrp/config.json.bak` | 0600 | 上一次可解析的配置 |
| `/etc/natfrp-config.snapshot` | 0600 | 配置镜像（在 `/etc/natfrp` 之外，用于恢复） |
| `/etc/natfrp/remote_pass` | 0600 | 远程管理明文密码（供页面显示） |
| `/etc/natfrp/webui.crt` | 0644 | WebUI 证书（带 SAN，825 天） |
| `/etc/natfrp/webui.key` | 0600 | WebUI 私钥 |
| `/etc/natfrp/FrpcWorkingDirectory` | 0755 | frpc 工作目录 |

- 日志默认走 syslog（`log_file` 为空，不写 flash）：`logread | grep -i natfrp`。
- 服务由 procd 托管，异常退出会自动拉起。

---

## 升级、迁移与卸载

### 本项目的版本之间

```sh
apk add --allow-untrusted --upgrade ./luci-app-natfrp-<新版本>.apk
natfrpctl apply --restart      # 需要时；服务已运行时才会重载
```

- apk 升级只执行 pre/post-upgrade（OpenWrt 未生成这两个钩子），**不会**跑 `postinst`；
  init 脚本启动时会清 LuCI 缓存，菜单/ACL 仍异常时手动 `rm -f /tmp/luci-indexcache.* && /etc/init.d/rpcd restart`。
- 每次成功保存都会刷新 `/etc/natfrp-config.snapshot`；若 `config.json` 丢失，
  `postinst` 会调用 `natfrpctl restore`，也可手动执行。
- 证书只在 SAN 或有效期不满足时重建；重建后需要重新下载导入。

### 从官方 `luci-app-natfrp` 3.1.9 迁移

- **同架构、同名包直接升级**：apk 只跑 pre/post-upgrade，旧包的 `pre-deinstall`
  （`rm -rf /etc/natfrp`）不会执行，token 与口令保留。
- **需要先 `apk del` 的情况**（例如架构名从 `aarch64_generic` 换到 `aarch64_cortex-a53`）：
  卸载会触发旧包清理，**必须先备份**：

```sh
cp /etc/natfrp/config.json /root/natfrp-config.backup
apk del luci-app-natfrp
apk add --allow-untrusted ./luci-app-natfrp-*.apk
mkdir -p /etc/natfrp && cp /root/natfrp-config.backup /etc/natfrp/config.json
chmod 600 /etc/natfrp/config.json
/etc/init.d/natfrp restart
```

### 卸载

- 卸载**不会**删除 `/etc/natfrp` 与 `/etc/natfrp-config.snapshot`（避免误删 token）。
- 需要彻底清除：

```sh
apk del luci-app-natfrp
rm -rf /etc/natfrp /etc/natfrp-config.snapshot /etc/config/natfrp
```

---

## 构建

### CI（`.github/workflows/build-apk.yml`）

1. **Node LTS**：`node --check` 页面脚本 + `scripts/test-inject.js`
   （用 Node `crypto` 校验 WebCrypto 兼容层，61 组向量）。
2. **最新稳定 Rust**：`cargo fmt --check` → `cargo test` → 交叉编译
   `aarch64-unknown-linux-musl`
   （`RUSTFLAGS="-C link-self-contained=yes -C linker=rust-lld"`，不使用 OpenWrt 的 Rust 体系）。
3. **OpenWrt 官方 SDK**：下载最新 snapshot SDK，只负责打包 APK。
4. **完整性校验**：从官方 CDN 取守护进程与 `frpc`，按 `scripts/vendor-sha256.txt` 逐项校验。
5. **发布**：push `main` 滚动更新 `latest` release；push `v*` tag 发布对应版本。

### 本地

```sh
ARCH=aarch64 ./scripts/build.sh              # 生成 src/luci-app-natfrp/files/natfrpctl-aarch64
HOST=root@192.168.1.1 ./scripts/deploy.sh    # 直接铺文件到设备（调试用）
```

`build.sh` 需在 Linux / WSL 下运行（rustup + musl 目标）。`deploy.sh` 只复制文件，
不经过 apk，适合快速迭代。

---

## 排障

| 现象 | 处理 |
|---|---|
| 菜单没出现 | `rm -f /tmp/luci-indexcache.* && /etc/init.d/rpcd restart`，重新登录 LuCI |
| 页面提示「配置文件读取失败」 | 不要删备份；先看 `natfrpctl restore` 能否恢复，或按提示人工检查 |
| 需要彻底重置配置 | `rm -rf /etc/natfrp /etc/natfrp-config.snapshot` 后重新保存 |
| 导入证书仍提示不安全 | 访问地址必须在证书 SAN 内；导入到「受信任的根证书颁发机构」；iOS 还需开启完全信任 |
| 关了 TLS 页面打不开 | 确认 `openssl-util` 已装、`webui_port > 1000`，并检查 `netstat` 是否有 4101 |
| 服务起不来 | `logread \| grep -i natfrp`；`/etc/init.d/natfrp status`；`pidof natfrp-service` |
| 忘记连接密码 | `natfrpctl secrets`（root/写权限），或在页面重新生成 |

---

## 已知限制

- 启动器自带 WebUI 的「新建/编辑隧道」内嵌
  `https://www.natfrp.com/_launcher/create-tunnel`，对方 `frame-ancestors` 只允许
  `'self'` 与 `https://launcher.natfrp.com`，局域网来源会被浏览器拦掉。请在 natfrp.com
  面板或远程管理页里编辑隧道。
- 远程管理目前只用 nonce 认证；上游支持 TOTP，但本包页面没有 TOTP 入口。
- 只发布 `aarch64_cortex-a53`；其他架构需自行调整交叉编译目标与 blob。
- 关闭 TLS 时 `relay` 每连接 fork 一个 `openssl s_client`，低端设备上开销明显
  （已有 32 连接上限与 15 秒超时）。
- 守护进程为预编译件，不含源码；本仓库不改写它。

---

## 安全说明

- **密钥位置**：`/etc/natfrp/config.json`（token、WebUI 密码，0600）、
  `/etc/natfrp/remote_pass`（远程管理明文密码，0600）。
- **参数暴露**：`set-token` 与 `remote-kdf` 通过命令行参数传递，执行瞬间会出现在
  `/proc/<pid>/cmdline`；单用户路由器影响很小，多用户设备需知悉。
- **上游 KDF**：远程管理密钥为 `PBKDF2-HMAC-SHA256`、固定盐、1 万次迭代，
  因此把「远程改配置/触发更新」默认关闭；请使用强密码并定期更换。
- **WebUI token**：浏览器控制通道使用 `webui_pass`（出现在连接 URL 的 query 中），
  仅在 TLS 或可信内网下使用；默认只建议在 LAN 内访问。
- **供应链**：守护进程二进制按 sha256 钉死；升级版本时请同步更新
  `scripts/vendor-sha256.txt`。

---

## 目录结构

```text
rust/                        natfrpctl 源码（std-only，官方工具链编译）
  main.rs                    子命令、配置转换、证书、relay 入口
  config.rs                  config.json 读写、.bak 与快照恢复
  relay.rs                   明文入口（TLS 终止、WebCrypto 注入）
  json.rs / sh.rs            极简 JSON 与 uci/进程封装
src/luci-app-natfrp/         单包
  Makefile                   包定义、安装规则、postinst
  htdocs/.../main.js         LuCI 页面
  root/etc/config/natfrp     uci 默认值
  root/etc/init.d/natfrp     procd 脚本
  root/usr/bin/natfrpctl-status  只读白名单入口
  root/usr/share/{luci,rpcd} 菜单与 ACL
  files/                     CI 生成：natfrpctl-<arch>、blobs/<arch>/*
scripts/                     本地构建、部署、兼容层自测、哈希清单
.github/workflows/           CI
```

---

## 更新日志

见 [CHANGELOG.md](CHANGELOG.md)。

## License

本仓库代码为 [MIT](LICENSE)；分发的 SakuraFrp 守护进程与 `frpc` 为 AGPL-3.0，
包元数据（`PKG_LICENSE`）标注为 `MIT AGPL-3.0-only`。SakuraFrp 名称与二进制归其各自权利人所有。
