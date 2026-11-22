# luci-app-natfrp (Rust backend)

SakuraFrp 启动器在 OpenWrt 上的集成：**Rust 后端 + 原生 LuCI JS 页面，不使用 Lua**。

面向 OpenWrt 25.12 及以后（apk 包管理，APK 格式）。CI 用最新快照版本的 SDK 打包，
默认产出 **aarch64_cortex-a53 单包 APK**（守护进程 + 后端 + 页面都在一个包里）。

## 特性

- **原生菜单注册**：`/usr/share/luci/menu.d` + ACL，不依赖 `luci-compat` 的 Lua 桥
- **后端 `natfrpctl`**：std-only Rust（无第三方 crate），静态链接，约 500 KB（aarch64）
  - `status` 运行状态、WebUI/远程管理信息（**不含口令**，只读账号可执行）
  - `secrets` 口令（仅写权限账号可执行，见 `natfrpctl-status` 只读入口）
  - `apply` 把 uci 配置转换成守护进程配置并让服务生效
  - `render` 仅生成配置（开机由 init 脚本调用）
  - `relay` 关闭 TLS 时的明文入口（含 WebCrypto 兼容层）
  - `webui-cert` 导出 WebUI 自签名证书
- **口令随机生成**：连接密码、远程管理 E2E 密码首次启用时从 `/dev/urandom` 生成 20 位，没有默认口令
- **配置不会静默丢失**：`config.json` 解析失败时先尝试 `.bak` 恢复，恢复不了就放弃写入并报错，
  而不是重写成空配置（token 与口令只存在这个文件里，重写＝掉线）
- **默认 HTTPS**：WebUI 默认走 TLS（自签名证书，页面提供下载入口）；要用明文入口需手动关闭 TLS，
  并依赖 `openssl-util`（已写进包依赖）
- **一键入口**：远程管理配置好后，页面主按钮直接跳 `https://www.natfrp.com/remote/v2`，否则打开本机 WebUI

## 构建

CI（`.github/workflows/build-apk.yml`）：

1. **Node LTS**：`node --check` 前端脚本 + `scripts/test-inject.js`（用 Node 的 crypto 校验兼容层）
2. **最新稳定版 Rust**：`cargo test` + 交叉编译 `aarch64-unknown-linux-musl`
   （`RUSTFLAGS="-C link-self-contained=yes -C linker=rust-lld"`，不使用 OpenWrt 的 rust 体系）
3. **OpenWrt 官方 SDK**：只负责把文件打成 APK，不参与 Rust 编译

产物是一个 `.apk`（release 里的文件名是 `luci-app-natfrp-<version>-r<release>.apk`），直接装：

```sh
apk add --allow-untrusted ./luci-app-natfrp-*.apk
```

> 从官方 `luci-app-natfrp` 3.1.9 升级前，**先备份 `/etc/natfrp/config.json`**。官方包的
> `pre-deinstall` 是 `rm -rf /etc/natfrp`，token 与口令只在那里。

本地构建：

```sh
ARCH=aarch64 ./scripts/build.sh     # 产物在 src/luci-app-natfrp/files/
HOST=root@192.168.1.1 ./scripts/deploy.sh   # 直接铺文件到设备调试
```

`build.sh` 需要放在 Linux / WSL 里跑（官方 rustup + musl 目标）。

CI 会按 `scripts/vendor-sha256.txt` 校验从官方 CDN 取的守护进程二进制；没收录的架构
只打印警告。新增架构时请先核对哈希再补进去。

## 首次使用

打开 **服务 → SakuraFrp 内网穿透**：

1. 勾「启用服务」，填访问密钥（服务端控制台获取），保存并应用
2. 需要远程管理就勾上，E2E 密码留空即可——首次会自动生成随机口令
3. 页面顶部显示连接密码与远程管理密码，各带复制按钮

## 排障

- 装完后菜单没出现：`rm -f /tmp/luci-indexcache.* && /etc/init.d/rpcd restart`，然后重新登录 LuCI
  （ACL 在登录时读取；包内 postinst 会自动做这一步，手动 `scp` 铺文件时才需要自己执行）。
- 页面提示「配置文件读取失败」：`/etc/natfrp/config.json` 被写坏了。程序不会覆盖它，而是先尝试
  `config.json.bak`；两者都不可用时按页面提示处理，**不要**先删备份。
- 关了 TLS 但页面打不开：检查 `openssl-util` 是否安装（`ls -l /usr/bin/openssl`），
  以及 `webui_port` 是否大于 1000。

## 已知限制

- 本地 WebUI 的「新建/编辑隧道」是嵌 `https://www.natfrp.com/_launcher/create-tunnel` 的 iframe，
  对方返回 `frame-ancestors 'self' https://launcher.natfrp.com`，局域网来源会被浏览器拦掉。
  请在 natfrp.com 面板或远程管理页面里编辑隧道。
- 守护进程二进制为预编译件，不含源码。本仓库只打包、不改写它；其许可证为 AGPL-3.0，
  本仓库自身代码为 MIT，分发时两者需分别遵守。
- 关闭 TLS 走明文入口时，`relay` 每建立一个连接会 fork 一个 `/usr/bin/openssl s_client`
  （一次页面加载十几个连接），低端设备上开销明显。

## 目录

```
rust/                       natfrpctl 源码（官方工具链编译）
src/luci-app-natfrp/        单包：Makefile + 页面 + 菜单/ACL + init 脚本
  files/                    CI 生成：natfrpctl-<arch>、blobs/<arch>/*
scripts/                    本地构建、部署、兼容层自测
.github/workflows/          CI
```

## License

本仓库代码为 MIT。SakuraFrp 名称与守护进程二进制归其各自权利人所有。
