# luci-app-natfrp (Rust backend)

SakuraFrp 启动器在 OpenWrt 上的集成：**Rust 后端 + 原生 LuCI JS 页面，不使用 Lua**。

面向 OpenWrt 25.12 及以后（apk 包管理，APK 格式），CI 用最新快照版本的 SDK 打包。

## 组成

| 包 | 架构 | 内容 |
|---|---|---|
| `natfrp-launcher` | 目标架构 | 守护进程 `natfrp-service` / `natfrp-frpc`（预编译二进制） |
| `natfrpctl` | 目标架构 | Rust 后端，状态查询、配置同步、明文入口 |
| `luci-app-natfrp` | all | LuCI 页面、菜单、ACL、uci 配置、procd 脚本 |

拆开的原因是：Lua 页面/菜单属于架构无关内容，换架构不用重打；Rust 后端用官方工具链交叉编译，不进入 OpenWrt 的 rust 构建体系。

## 特性

- **原生菜单注册**：`/usr/share/luci/menu.d` + ACL，不依赖 `luci-compat` 的 Lua 桥
- **后端 `natfrpctl`**：std-only Rust（无第三方 crate），静态链接，约 490 KB
  - `status` 运行状态、WebUI/远程管理信息
  - `apply` 把 uci 配置转换成守护进程配置并让服务生效
  - `render` 仅生成配置（开机由 init 脚本调用）
  - `relay` 关闭 TLS 时的明文入口
  - `webui-cert` 导出 WebUI 自签名证书
- **口令随机生成**：连接密码、远程管理 E2E 密码首次启用时从 `/dev/urandom` 生成 20 位，没有任何默认口令
- **一键入口**：远程管理配置好后，页面主按钮直接跳 `https://www.natfrp.com/remote/v2`，否则打开本机 WebUI
- **可选明文入口**：关闭 TLS 时由 `natfrpctl relay` 在对外端口提供 `http://`，并给页面注入 WebCrypto 兼容层（启动器 UI 的登录依赖 `crypto.subtle`，只在安全上下文可用）

## 构建

CI（`.github/workflows/build-apk.yml`）三步：

1. **最新 Node.js**：`node --check` 前端脚本 + `scripts/test-inject.js`（用 Node 的 crypto 校验兼容层）
2. **最新稳定版 Rust**：`cargo test` + 交叉编译 `aarch64-unknown-linux-musl`
   （`RUSTFLAGS="-C link-self-contained=yes -C linker=rust-lld"`，无需 OpenWrt 工具链）
3. **OpenWrt 官方 SDK**：只负责把文件打成 APK，不参与 Rust 编译

产物：三个 `.apk`，直接 `apk add --allow-untrusted ./xxx.apk`。

本地构建后端：

```sh
./scripts/build.sh            # 默认 aarch64，产物在 src/natfrpctl/files/
ARCH=x86_64 ./scripts/build.sh
```

开发时直接铺文件到设备：

```sh
HOST=root@192.168.1.1 ./scripts/deploy.sh
```

## 首次使用

```sh
apk add --allow-untrusted ./natfrp-launcher-*.apk ./natfrpctl-*.apk ./luci-app-natfrp-*.apk
```

打开 **服务 → SakuraFrp 内网穿透**：

1. 勾「启用服务」，填访问密钥（服务端控制台获取），保存并应用
2. 需要远程管理就勾上，E2E 密码留空即可——首次会自动生成随机口令
3. 页面顶部会显示连接密码和远程管理密码，各带复制按钮

## 已知限制

- 本地 WebUI 里的「新建/编辑隧道」是嵌 `https://www.natfrp.com/_launcher/create-tunnel` 的 iframe，
  对方返回 `frame-ancestors 'self' https://launcher.natfrp.com`，任何局域网来源都会被浏览器拦掉。
  请在 natfrp.com 面板或远程管理页面里编辑隧道。
- 守护进程二进制为预编译件，不含源码；`natfrp-launcher` 只负责安装。

## 目录

```
src/natfrpctl/            Rust 后端（Makefile 只安装预编译二进制）
src/natfrpctl/src/        源码 + inject.js（WebCrypto 兼容层）
src/luci-app-natfrp/      LuCI 页面与运行脚本
src/natfrp-launcher/      守护进程二进制包
scripts/                  本地构建、部署、自测
.github/workflows/        CI
```

## License

本仓库代码为 MIT。SakuraFrp 名称与守护进程二进制归其各自权利人所有。
