//! natfrpctl — SakuraFrp 启动器的控制后端 / LuCI 页面的数据来源。
//!
//! 子命令：
//!   status     运行状态（JSON，不含口令；只读账号也可执行）
//!   secrets    WebUI / 远程管理口令（需要写权限）
//!   set-token  写入访问密钥（不经 uci，避免只读账号读到明文）
//!   restore    配置丢失时从快照恢复（升级/误删）
//!   apply      将 uci 配置同步到守护进程配置并让服务生效
//!   render     仅同步配置文件（开机由 init 脚本调用）
//!   relay      明文 HTTP 入口（含 WebCrypto 兼容层）
//!   webui-cert 输出 WebUI 自签名证书（PEM），供浏览器导入信任
//!   version    输出本程序版本
//!
//! 输出统一为一行 JSON：成功 {"ok":true,...}，失败 {"ok":false,"error":"..."} 且退出码 1。

mod config;
mod json;
mod relay;
mod sh;

use json::J;
use std::path::Path;

const SERVICE_NAME: &str = "natfrp-service";
const SERVICE_BIN: &str = "/usr/bin/natfrp-service";
const INITD: &str = "/etc/init.d/natfrp";
const RCD_LINK: &str = "/etc/rc.d/S90natfrp";
const CERT_PATH: &str = "/etc/natfrp/webui.crt";
const CERT_KEY_PATH: &str = "/etc/natfrp/webui.key";
/// Apple 平台对 TLS 证书强制 ≤825 天，再长导入信任也不生效
const CERT_DAYS: &str = "825";
const REMOTE_PASS_FILE: &str = "/etc/natfrp/remote_pass";
const VERSION_CACHE: &str = "/etc/natfrp/ctl_version";
const RELAY_OFFSET: i64 = 1000;
const DEFAULT_WEBUI_PORT: i64 = 4101;
const DEFAULT_WEBUI_HOST: &str = "0.0.0.0";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("help");

    let code = match cmd {
        "status" => cmd_status(),
        "secrets" => cmd_secrets(),
        "set-token" => cmd_set_token(&args[1..]),
        "restore" => cmd_restore(),
        "apply" => cmd_apply(&args[1..]),
        "render" => cmd_render_only(),
        "relay" => cmd_relay(),
        "webui-cert" => cmd_webui_cert(),
        "version" | "-V" | "--version" => {
            print(&ok_json(Some((
                "version",
                J::Str(env!("CARGO_PKG_VERSION").into()),
            ))));
            0
        }
        _ => {
            print(
                "用法: natfrpctl {status|secrets|set-token|restore|apply|render|relay|webui-cert|version}
  status       运行状态与 WebUI 入口信息
  secrets      口令（WebUI 连接密码 / 远程管理密码）
  set-token    写入访问密钥（参数为密钥本身；--clear 清除）
  restore      配置丢失时从快照恢复
  apply        应用 uci 配置并重启服务（--restart 强制重载）
  render       仅生成守护进程配置文件
  relay        明文 HTTP 入口（关闭 WebUI TLS 时使用）
  webui-cert   输出 WebUI 证书（PEM）
  version      版本号",
            );
            0
        }
    };
    std::process::exit(code);
}

fn print(body: &str) {
    println!("{}", body);
}

/// 写入系统日志，便于事后排查配置应用过程
fn note(msg: &str) {
    sh::log("info", msg);
}

fn ok_json(extra: Option<(&str, J)>) -> String {
    let mut out = J::obj();
    out.set("ok", J::Bool(true));
    if let Some((k, v)) = extra {
        out.set(k, v);
    }
    out.render()
}

fn err_json(msg: &str) -> String {
    let mut out = J::obj();
    out.set("ok", J::Bool(false));
    out.set("error", J::Str(msg.to_string()));
    out.render()
}

// --------------------------------------------------------------- uci 快照

/// 一次性读出的 uci 配置快照。取不到时逐键回退，保证在非常规环境下也能工作。
struct Uci {
    map: Vec<(String, String)>,
}

impl Uci {
    fn new() -> Uci {
        Uci {
            map: sh::uci_show(),
        }
    }

    fn get(&self, name: &str) -> String {
        if let Some((_, v)) = self.map.iter().find(|(k, _)| k == name) {
            return v.clone();
        }
        // `uci show` 整份失败时才逐键回退，避免把「键确实不存在」也变成一次 fork
        if self.map.is_empty() {
            return sh::uci_get(name);
        }
        String::new()
    }

    fn flag(&self, name: &str) -> bool {
        self.get(name) == "1"
    }

    /// 布尔值缺失时按给定默认值处理（老配置迁移时很重要）
    fn flag_or(&self, name: &str, def: &str) -> bool {
        let raw = self.get(name);
        let v = if raw.is_empty() { def } else { raw.as_str() };
        v == "1"
    }

    fn int_or(&self, name: &str, def: i64) -> i64 {
        self.get(name).parse::<i64>().unwrap_or(def)
    }

    fn str_or(&self, name: &str, def: &str) -> String {
        let raw = self.get(name);
        if raw.is_empty() {
            def.to_string()
        } else {
            raw
        }
    }
}

/// 生成随机口令（去掉易混淆字符 l/1/O/0）。
/// 用拒绝采样，避免 `byte % 57` 带来的模偏差。
fn random_password(len: usize) -> Result<String, String> {
    use std::io::Read;

    const ALPHABET: &[u8] = b"abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let limit = 256 - (256 % ALPHABET.len());

    let mut file = std::fs::File::open("/dev/urandom")
        .map_err(|e| format!("打开 /dev/urandom 失败: {}", e))?;
    let mut out = String::with_capacity(len);
    let mut buf = [0u8; 64];

    while out.len() < len {
        file.read_exact(&mut buf)
            .map_err(|e| format!("读取随机数失败: {}", e))?;
        for b in buf.iter() {
            let v = *b as usize;
            if v < limit {
                out.push(ALPHABET[v % ALPHABET.len()] as char);
                if out.len() == len {
                    break;
                }
            }
        }
    }

    Ok(out)
}

fn save_remote_pass(pass: &str) -> Result<(), String> {
    let body = format!("{}\n", pass);
    std::fs::write(REMOTE_PASS_FILE, body.as_bytes())
        .map_err(|e| format!("写入 {} 失败: {}", REMOTE_PASS_FILE, e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(REMOTE_PASS_FILE, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn load_remote_pass() -> String {
    std::fs::read_to_string(REMOTE_PASS_FILE)
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

// --------------------------------------------------------------- status

fn pid_of() -> Option<u32> {
    let (out, ok) = sh::run("pidof", &[SERVICE_NAME]);
    if !ok {
        return None;
    }
    out.split_whitespace().next().and_then(|p| p.parse().ok())
}

fn service_enabled() -> bool {
    Path::new(RCD_LINK).exists()
}

fn binary_stamp() -> String {
    use std::time::UNIX_EPOCH;
    std::fs::metadata(SERVICE_BIN)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}

/// 从缓存读版本号（只在二进制没变时可信）
fn cached_version(stamp: &str) -> Option<String> {
    let text = std::fs::read_to_string(VERSION_CACHE).ok()?;
    let (s, v) = text.split_once('\n')?;
    if s.trim() == stamp {
        let v = v.trim();
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    None
}

fn store_version(stamp: &str, version: &str) {
    let _ = std::fs::write(
        VERSION_CACHE,
        format!("{}\n{}\n", stamp, version).as_bytes(),
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(VERSION_CACHE, std::fs::Permissions::from_mode(0o600));
    }
}

/// 守护进程版本号。优先取 `config.json` 里守护进程自己写的 version 字段，
/// 其次取缓存，最后才真去执行一次 `natfrp-service -V`（每次约 6 MB 的进程启动，
/// 不适合放进每轮轮询里）。
fn service_version(cfg: &J) -> String {
    if let Some(v) = cfg.str_of("version") {
        if !v.is_empty() {
            let stamp = binary_stamp();
            if cached_version(&stamp).as_deref() != Some(v) {
                store_version(&stamp, v);
            }
            return v.to_string();
        }
    }

    let stamp = binary_stamp();
    if let Some(v) = cached_version(&stamp) {
        return v;
    }

    let (out, ok) = sh::run_env(SERVICE_BIN, &["-V"], &[("NATFRP_SERVICE_WD", "/tmp")]);
    if !ok {
        return String::new();
    }
    let version = out.split_whitespace().next().unwrap_or("").to_string();
    if !version.is_empty() {
        store_version(&stamp, &version);
    }
    version
}

fn cmd_status() -> i32 {
    cmd_status_with(&[])
}

fn cmd_status_with(extra: &[(&str, J)]) -> i32 {
    let uci = Uci::new();
    let pid = pid_of();

    let (cfg, cfg_error) = match config::load() {
        Ok(c) => (c, None::<String>),
        Err(e) => (J::obj(), Some(e)),
    };

    let token_set = cfg.str_of("token").map(|t| !t.is_empty()).unwrap_or(false);

    // WebUI 对外暴露的地址取自 uci
    let webui_on = uci.flag("main.webui");
    let port = uci.int_or("main.webui_port", DEFAULT_WEBUI_PORT);
    let host = uci.str_or("main.webui_host", DEFAULT_WEBUI_HOST);

    let mut webui = J::obj();
    webui.set("enabled", J::Bool(webui_on && port > 0));
    webui.set("host", J::Str(host));
    webui.set("port", J::Num(port.to_string()));
    webui.set("tls", J::Bool(uci.flag_or("main.webui_tls", "1")));
    webui.set(
        "pass_set",
        J::Bool(
            cfg.str_of("webui_pass")
                .map(|p| !p.is_empty())
                .unwrap_or(false),
        ),
    );

    let mut remote = J::obj();
    let remote_on = uci.flag("main.remote_mgmt");
    remote.set("enabled", J::Bool(remote_on));
    remote.set(
        "keyed",
        J::Bool(
            cfg.str_of("remote_management_key")
                .map(|k| !k.is_empty())
                .unwrap_or(false),
        ),
    );
    remote.set(
        "allow_config",
        J::Bool(
            cfg.bool_of("remote_management_allow_config")
                .unwrap_or(true),
        ),
    );
    remote.set(
        "allow_update",
        J::Bool(
            cfg.bool_of("remote_management_allow_update")
                .unwrap_or(true),
        ),
    );
    remote.set(
        "auth_mode",
        J::Str(
            cfg.str_of("remote_management_auth_mode")
                .unwrap_or("nonce")
                .to_string(),
        ),
    );
    remote.set("pass_set", J::Bool(!load_remote_pass().is_empty()));

    let mut out = J::obj();
    out.set("ok", J::Bool(true));
    out.set("running", J::Bool(pid.is_some()));
    out.set(
        "pid",
        match pid {
            Some(p) => J::Num(p.to_string()),
            None => J::Null,
        },
    );
    out.set("enabled", J::Bool(service_enabled()));
    out.set("version", J::Str(service_version(&cfg)));
    out.set("token_set", J::Bool(token_set));
    out.set("webui", webui);
    out.set("remote", remote);
    out.set(
        "config_error",
        match cfg_error {
            Some(e) => J::Str(e),
            None => J::Null,
        },
    );

    for (k, v) in extra {
        out.set(k, v.clone());
    }

    print(&out.render());
    0
}

/// 口令单独走一个子命令：只授予写权限的会话可执行，避免只读账号从 status 里读到明文
fn cmd_secrets() -> i32 {
    let cfg = match config::load() {
        Ok(c) => c,
        Err(e) => {
            print(&err_json(&e));
            return 1;
        }
    };

    let mut out = J::obj();
    out.set("ok", J::Bool(true));
    out.set(
        "webui_pass",
        J::Str(cfg.str_of("webui_pass").unwrap_or("").to_string()),
    );
    out.set("remote_pass", J::Str(load_remote_pass()));
    print(&out.render());
    0
}

// --------------------------------------------------------------- token / restore

/// 直接写 config.json，不经过 uci。
///
/// 走 uci 的话，密钥在「保存」到「apply」之间会以明文留在 `/etc/config/natfrp`，
/// 而只读 LuCI 账号持有 `uci: natfrp` 的读权限；改成这里写入后，uci 里不再出现 token。
/// 代价是参数会短暂出现在 `/proc/<pid>/cmdline`（仅本机进程可见，且窗口是毫秒级）。
fn cmd_set_token(args: &[String]) -> i32 {
    let clear = args.iter().any(|a| a == "--clear");
    let token = if clear {
        String::new()
    } else {
        match args.first() {
            Some(t) if !t.trim().is_empty() => t.trim().to_string(),
            _ => {
                print(&err_json(
                    "用法: natfrpctl set-token <访问密钥> | set-token --clear",
                ));
                return 1;
            }
        }
    };

    if !clear {
        if token.len() > 512 {
            print(&err_json("访问密钥过长"));
            return 1;
        }
        if token.chars().any(|c| c.is_control() || c.is_whitespace()) {
            print(&err_json("访问密钥包含非法字符（空白或控制字符）"));
            return 1;
        }
    }

    let mut cfg = match config::load() {
        Ok(c) => c,
        Err(e) => {
            print(&err_json(&e));
            return 1;
        }
    };
    config::set_str(&mut cfg, "token", &token);
    if let Err(e) = config::save(&cfg) {
        print(&err_json(&e));
        return 1;
    }

    // 迁移：旧版本可能把 token 留在 uci 里
    sh::uci_delete("main.token");
    sh::uci_commit();
    note(if clear {
        "access token cleared"
    } else {
        "access token updated"
    });

    print(&ok_json(Some(("token_set", J::Bool(!token.is_empty())))));
    0
}

/// 配置丢失时从 `/etc/natfrp-config.snapshot` 恢复（升级时官方包会 rm -rf /etc/natfrp）。
fn cmd_restore() -> i32 {
    match config::restore_if_missing() {
        Ok(restored) => {
            print(&ok_json(Some(("restored", J::Bool(restored)))));
            0
        }
        Err(e) => {
            sh::log("err", &format!("restore: {}", e));
            print(&err_json(&e));
            1
        }
    }
}

// --------------------------------------------------------------- render

/// WebUI 是否对外提供 HTTPS。关闭时守护进程只监听回环，
/// 由 `natfrpctl relay` 在对外端口提供明文入口。
fn uci_tls_enabled(uci: &Uci) -> bool {
    uci.flag_or("main.webui_tls", "1")
}

/// 关闭 TLS 时守护进程内部使用的端口
fn internal_port(port: i64) -> i64 {
    if port > 0 && port + RELAY_OFFSET <= 65535 {
        port + RELAY_OFFSET
    } else if port > RELAY_OFFSET {
        port - RELAY_OFFSET
    } else {
        port
    }
}

/// uci → /etc/natfrp/config.json。返回 uci 是否被修改（密钥字段被清空）。
fn render_config() -> Result<bool, String> {
    let uci = Uci::new();
    let mut uci_dirty = false;

    let token = uci.get("main.token");
    let webui_on = uci.flag("main.webui");
    let tls_on = uci_tls_enabled(&uci);
    let check_update = uci.flag("main.check_update");
    let remote_on = uci.flag("main.remote_mgmt");
    let webui_pass = uci.get("main.webui_pass");
    let remote_pass = uci.get("main.remote_mgmt_pass");

    let mut port = uci.int_or("main.webui_port", DEFAULT_WEBUI_PORT);
    if port <= 0 || port > 65535 {
        port = DEFAULT_WEBUI_PORT;
    }
    let host = uci.str_or("main.webui_host", DEFAULT_WEBUI_HOST);

    // 明文入口与守护进程各占一个端口。端口太小时偏移量无处可用，会让两者抢同一端口，
    // 表现为 procd 反复 respawn。宁可拒绝配置，也不接受一个必然冲突的组合。
    if webui_on && !tls_on && port <= RELAY_OFFSET {
        return Err(format!(
            "关闭 TLS 时监听端口必须大于 {}（当前为 {}），否则明文入口会和守护进程抢同一个端口",
            RELAY_OFFSET, port
        ));
    }

    let mut cfg = config::load()?;

    // 日志走 syslog，避免写 flash
    config::set_str(&mut cfg, "log_file", "");
    config::set_bool(&mut cfg, "log_stdout", true);

    if !token.is_empty() {
        config::set_str(&mut cfg, "token", &token);
        sh::uci_delete("main.token");
        uci_dirty = true;
    }

    if webui_on {
        if tls_on {
            config::set_int(&mut cfg, "webui_port", port);
            config::set_str(&mut cfg, "webui_host", &host);
        } else {
            config::set_int(&mut cfg, "webui_port", internal_port(port));
            config::set_str(&mut cfg, "webui_host", "127.0.0.1");
        }
        config::set_int(
            &mut cfg,
            "update_interval",
            if check_update { 604800 } else { -1 },
        );
        if !webui_pass.is_empty() {
            config::set_str(&mut cfg, "webui_pass", &webui_pass);
            sh::uci_delete("main.webui_pass");
            uci_dirty = true;
        } else if cfg
            .str_of("webui_pass")
            .map(|v| v.is_empty())
            .unwrap_or(true)
        {
            // 首次启用且没有现成密码：随机生成，避免出现固定默认口令
            let generated = random_password(20)?;
            config::set_str(&mut cfg, "webui_pass", &generated);
            note("已为 WebUI 生成随机连接密码");
        }
    } else {
        config::set_int(&mut cfg, "webui_port", -1);
        config::set_int(&mut cfg, "update_interval", -1);
    }

    // 自己生成带 SAN 的证书：守护进程自签的那张没有 subjectAltName 且有效期 10 年，
    // Chrome 只认 SAN（导入后仍报名称不匹配）、iOS/macOS 拒绝 >825 天的证书。
    // 关闭 TLS 时守护进程仍在回环上提供 HTTPS（relay 负责解 TLS），同样需要证书。
    if webui_on {
        let cert_host = if tls_on { host.as_str() } else { "127.0.0.1" };
        match ensure_webui_cert(cert_host) {
            Ok(()) => {
                config::set_str(&mut cfg, "webui_cert", CERT_PATH);
                config::set_str(&mut cfg, "webui_cert_key", CERT_KEY_PATH);
            }
            Err(e) => note(&format!("WebUI 证书生成失败（将由守护进程自签）: {}", e)),
        }
    }

    // 开机自动启动的隧道：uci 里用逗号/空格分隔的隧道 ID。
    // 这里必须严格收数字：以前把任意字符串都当数字塞进 JSON，会写出 `[123,abc]` 这种
    // 非法文档，守护进程启动即报 "invalid character"，随后按空配置运行（等于静默登出）。
    let auto_tunnels = uci.get("main.auto_start_tunnels");
    if !auto_tunnels.trim().is_empty() {
        let mut ids: Vec<J> = Vec::new();
        let mut bad: Vec<String> = Vec::new();
        for part in auto_tunnels.split(|c: char| c == ',' || c.is_whitespace()) {
            let t = part.trim();
            if t.is_empty() {
                continue;
            }
            match t.parse::<u64>() {
                Ok(n) => ids.push(J::Num(n.to_string())),
                Err(_) => bad.push(t.to_string()),
            }
        }
        if !bad.is_empty() {
            return Err(format!(
                "自动启动隧道只能填隧道 ID（数字），以下内容无法识别：{}",
                bad.join("、")
            ));
        }
        cfg.set("auto_start_tunnels", J::Arr(ids));
    }

    config::set_bool(&mut cfg, "remote_management", remote_on);

    // 远程端能做什么：默认沿用守护进程的取值（都允许）；uci 里显式配置时才覆盖。
    if !uci.get("main.remote_allow_config").is_empty() {
        config::set_bool(
            &mut cfg,
            "remote_management_allow_config",
            uci.flag("main.remote_allow_config"),
        );
    }
    if !uci.get("main.remote_allow_update").is_empty() {
        config::set_bool(
            &mut cfg,
            "remote_management_allow_update",
            uci.flag("main.remote_allow_update"),
        );
    }

    let has_key = cfg
        .str_of("remote_management_key")
        .map(|k| !k.is_empty())
        .unwrap_or(false);

    if remote_on && !remote_pass.is_empty() {
        let pass = remote_pass.as_str();
        let (key, ok) = sh::run_env(
            SERVICE_BIN,
            &["remote-kdf", pass],
            &[("NATFRP_SERVICE_WD", config::DIR)],
        );
        if ok && !key.is_empty() {
            config::set_str(&mut cfg, "remote_management_key", &key);
            sh::uci_delete("main.remote_mgmt_pass");
            uci_dirty = true;
            let _ = save_remote_pass(pass);
        } else {
            return Err("E2E 密码转换失败，请检查密码内容".to_string());
        }
    } else if remote_on && !has_key {
        // 首次启用远程管理：随机生成 E2E 口令并派生密钥
        let generated = random_password(20)?;
        let (key, ok) = sh::run_env(
            SERVICE_BIN,
            &["remote-kdf", generated.as_str()],
            &[("NATFRP_SERVICE_WD", config::DIR)],
        );
        if ok && !key.is_empty() {
            config::set_str(&mut cfg, "remote_management_key", &key);
            let _ = save_remote_pass(&generated);
            note("已为远程管理生成随机 E2E 密码");
        } else {
            return Err("E2E 密码生成失败".to_string());
        }
    } else if !remote_on {
        // 关闭远程管理时不保留明文口令文件
        let _ = std::fs::remove_file(REMOTE_PASS_FILE);
    }

    if uci_dirty {
        sh::uci_commit();
    }
    config::save(&cfg)?;
    Ok(uci_dirty)
}

fn cmd_render_only() -> i32 {
    match render_config() {
        Ok(_) => {
            print(&ok_json(None));
            0
        }
        Err(e) => {
            print(&err_json(&e));
            1
        }
    }
}

// --------------------------------------------------------------- apply

fn cmd_apply(args: &[String]) -> i32 {
    // 只改 token 时 config.json 的前后内容都已是新值，cfg_changed 会是 false，
    // 服务不会重载。页面在 set-token 之后显式传 --restart 保证生效。
    let force_restart = args.iter().any(|a| a == "--restart");

    let before = std::fs::read_to_string(config::PATH).unwrap_or_default();
    let uci_dirty = match render_config() {
        Ok(d) => d,
        Err(e) => {
            sh::log("err", &format!("apply aborted: {}", e));
            print(&err_json(&e));
            return 1;
        }
    };
    let after = std::fs::read_to_string(config::PATH).unwrap_or_default();
    let cfg_changed = before != after;

    let enabled = Uci::new().flag("main.enabled");
    let linked = service_enabled();
    note(&format!(
        "apply: enabled={} linked={} cfg_changed={} uci_dirty={}",
        enabled, linked, cfg_changed, uci_dirty
    ));

    if enabled && !linked {
        let (_, ok) = sh::run(INITD, &["enable"]);
        note(&format!("service enable -> {}", ok));
    } else if !enabled && linked {
        let (_, ok) = sh::run(INITD, &["disable"]);
        note(&format!("service disable -> {}", ok));
    }

    let running = pid_of().is_some();
    let mut restarted = false;
    if !enabled {
        if running {
            let (_, ok) = sh::run(INITD, &["stop"]);
            restarted = true;
            note(&format!("service stop -> {}", ok));
        }
    } else if !running {
        let (_, ok) = sh::run(INITD, &["start"]);
        restarted = true;
        note(&format!("service start -> {}", ok));
    } else if cfg_changed || uci_dirty || force_restart {
        // 配置变了才重启，避免每次保存都断流；--restart 用于只改 token 的场景
        let (_, ok) = sh::run(INITD, &["reload"]);
        restarted = true;
        note(&format!("service reload -> {}", ok));
    }

    cmd_status_with(&[
        ("restarted", J::Bool(restarted)),
        ("config_changed", J::Bool(cfg_changed || uci_dirty)),
    ])
}
// ---------------------------------------------------------------- relay

/// 明文 HTTP 入口：守护进程只提供 HTTPS，关闭 TLS 时让它留在回环上，
/// 由这里对外提供 http://，并在首页注入 WebCrypto 兼容层。
fn cmd_relay() -> i32 {
    let uci = Uci::new();

    if uci_tls_enabled(&uci) {
        note("relay: TLS 已启用，无需明文入口");
        return 0;
    }
    if !uci.flag("main.webui") {
        note("relay: WebUI 未启用");
        return 0;
    }

    let host = uci.str_or("main.webui_host", DEFAULT_WEBUI_HOST);
    let port = uci.int_or("main.webui_port", DEFAULT_WEBUI_PORT);
    if port <= RELAY_OFFSET || port > 65535 {
        sh::log(
            "err",
            &format!(
                "relay: 端口 {} 不可用（关闭 TLS 时需大于 {}）",
                port, RELAY_OFFSET
            ),
        );
        return 1;
    }

    // 明文入口要把 HTTPS 转成 HTTP，靠外部 openssl 命令行；缺它时每条连接都会静默断开，
    // 表现是「页面一直空白」。启动时先明确报错，便于一眼看出是依赖缺失。
    if !relay::openssl_available() {
        sh::log(
            "err",
            "relay: 缺少 /usr/bin/openssl（请安装 openssl-util），明文 WebUI 无法建立连接",
        );
        return 1;
    }

    let bind = format!("{}:{}", format_host(&host), port);
    let upstream = format!("127.0.0.1:{}", internal_port(port));

    match relay::serve(&bind, &upstream) {
        Ok(()) => 0,
        Err(e) => {
            sh::log("err", &format!("relay: {}", e));
            eprintln!("{}", err_json(&e));
            1
        }
    }
}

/// `::` / `::1` 这类 IPv6 字面量必须加方括号才能拼成合法的 host:port
fn format_host(host: &str) -> String {
    let h = host.trim();
    if h.contains(':') && !h.starts_with('[') {
        format!("[{}]", h)
    } else {
        h.to_string()
    }
}

// ---------------------------------------------------------------- cert

/// 输出 WebUI 使用的自签名证书（PEM），供用户导入浏览器信任库。
fn cmd_webui_cert() -> i32 {
    let cfg = match config::load() {
        Ok(c) => c,
        Err(e) => {
            print(&err_json(&e));
            return 1;
        }
    };
    let path = cfg
        .str_of("webui_cert")
        .map(|p| p.to_string())
        .filter(|p| !p.is_empty())
        .map(|p| {
            if p.starts_with('/') {
                p
            } else {
                format!("{}/{}", config::DIR, p)
            }
        })
        .unwrap_or_else(|| CERT_PATH.to_string());

    let pem = match std::fs::read_to_string(&path) {
        Ok(p) => p,
        Err(e) => {
            print(&err_json(&format!("读取证书 {} 失败: {}", path, e)));
            return 1;
        }
    };

    let mut out = J::obj();
    out.set("ok", J::Bool(true));
    out.set("path", J::Str(path));
    out.set("pem", J::Str(pem));
    print(&out.render());
    0
}

/// 确保证书存在、未过期、且 SAN 覆盖当前访问地址；否则重新生成。
///
/// 守护进程在 `webui_cert` 为空时会自签一张 **无 SAN、有效期 10 年** 的证书：
/// Chrome/Edge/Safari 只认 SAN（导入后仍报名称不匹配），iOS/macOS 则强制
/// ≤825 天。自己生成一张 SAN 正确、825 天的证书，并把路径写进配置，服务
/// 就会加载它。重新生成只在缺失/SAN 不覆盖时发生，避免每次 apply 都让用户
/// 重新导入。
fn ensure_webui_cert(host: &str) -> Result<(), String> {
    if !is_executable("/usr/bin/openssl") {
        return Err("/usr/bin/openssl 不可执行（请安装 openssl-util）".to_string());
    }

    let sans = desired_sans(host);
    if cert_is_current(CERT_PATH, CERT_KEY_PATH, &sans) {
        return Ok(());
    }
    generate_cert(&sans)
}

/// 证书里必须覆盖的名字/IP：本机回环 + 实际监听地址（通配监听时枚举本机私网地址）。
fn desired_sans(host: &str) -> Vec<String> {
    let mut out = vec!["DNS:localhost".to_string(), "IP:127.0.0.1".to_string()];
    let h = host.trim();

    let wildcard = h.is_empty() || h == "0.0.0.0" || h == "::";
    if !wildcard {
        if let Ok(ip) = h.parse::<std::net::IpAddr>() {
            out.push(format!("IP:{}", ip));
        } else if !h.contains(':') {
            out.push(format!("DNS:{}", h));
        }
    } else {
        for ip in local_addresses() {
            out.push(format!("IP:{}", ip));
        }
    }

    out.sort();
    out.dedup();
    out
}

/// 本机私网地址（`ip -o addr show scope global`，失败时用 UDP connect 探测默认出口）。
fn local_addresses() -> Vec<String> {
    let mut out = Vec::new();

    let (text, ok) = sh::run("ip", &["-o", "addr", "show", "scope", "global"]);
    if ok {
        for line in text.lines() {
            let mut parts = line.split_whitespace();
            while let Some(tok) = parts.next() {
                if tok != "inet" && tok != "inet6" {
                    continue;
                }
                if let Some(addr) = parts.next() {
                    let addr = addr
                        .split('/')
                        .next()
                        .unwrap_or("")
                        .split('%')
                        .next()
                        .unwrap_or("");
                    if is_private_addr(addr) {
                        out.push(addr.to_string());
                    }
                }
            }
        }
    }

    if out.is_empty() {
        for probe in ["8.8.8.8:53", "[2001:4860:4860::8888]:53"] {
            let bind = if probe.starts_with('[') {
                "[::]:0"
            } else {
                "0.0.0.0:0"
            };
            if let Ok(sock) = std::net::UdpSocket::bind(bind) {
                // UDP connect 不发包，只让内核选出默认路由的源地址
                if sock.connect(probe).is_ok() {
                    if let Ok(local) = sock.local_addr() {
                        let ip = local.ip().to_string();
                        if is_private_addr(&ip) {
                            out.push(ip);
                        }
                    }
                }
            }
        }
    }

    out.sort();
    out.dedup();
    out
}

/// 私网 / CGNAT / ULA 才算“用户可能用来访问管理页的地址”。
/// 公网 WAN 地址与链路本地地址不进证书，避免把证书分享出去时泄露信息。
/// 私网 / ULA 才算“用户可能用来访问管理页的地址”。
/// 公网 WAN、CGNAT（100.64/10）与链路本地地址不进证书：它们不代表 LAN 访问入口，
/// 而且会随运营商/接口变化，导致证书反复重新生成、用户需要重新导入。
fn is_private_addr(addr: &str) -> bool {
    use std::net::IpAddr;

    match addr.parse::<IpAddr>() {
        Ok(IpAddr::V4(v4)) => v4.is_private() || v4.is_loopback(),
        Ok(IpAddr::V6(v6)) => {
            let seg = v6.segments();
            v6.is_loopback() || (seg[0] & 0xfe00) == 0xfc00
        }
        Err(_) => false,
    }
}

fn cert_is_current(cert: &str, key: &str, sans: &[String]) -> bool {
    if !Path::new(cert).exists() || !Path::new(key).exists() {
        return false;
    }

    let (_, alive) = sh::run(
        "/usr/bin/openssl",
        &["x509", "-in", cert, "-noout", "-checkend", "86400"],
    );
    if !alive {
        return false;
    }

    let (text, ok) = sh::run(
        "/usr/bin/openssl",
        &["x509", "-in", cert, "-noout", "-ext", "subjectAltName"],
    );
    if !ok {
        return false;
    }

    sans.iter().all(|s| {
        if let Some(v) = s.strip_prefix("DNS:") {
            text.contains(&format!("DNS:{}", v))
        } else if let Some(v) = s.strip_prefix("IP:") {
            text.contains(&format!("IP Address:{}", v))
        } else {
            false
        }
    })
}

fn generate_cert(sans: &[String]) -> Result<(), String> {
    let key_tmp = format!("{}.tmp", CERT_KEY_PATH);
    let crt_tmp = format!("{}.tmp", CERT_PATH);
    let _ = std::fs::remove_file(&key_tmp);
    let _ = std::fs::remove_file(&crt_tmp);

    let san = format!("subjectAltName={}", sans.join(","));
    let (out, ok) = sh::run(
        "/usr/bin/openssl",
        &[
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-sha256",
            "-nodes",
            "-keyout",
            &key_tmp,
            "-out",
            &crt_tmp,
            "-days",
            CERT_DAYS,
            "-subj",
            "/CN=SakuraFrp Launcher WebUI",
            "-addext",
            &san,
        ],
    );
    if !ok {
        let _ = std::fs::remove_file(&key_tmp);
        let _ = std::fs::remove_file(&crt_tmp);
        return Err(if out.is_empty() {
            "openssl req 执行失败".to_string()
        } else {
            out
        });
    }

    set_mode(&key_tmp, 0o600);
    set_mode(&crt_tmp, 0o644);
    std::fs::rename(&key_tmp, CERT_KEY_PATH).map_err(|e| format!("安装私钥失败: {}", e))?;
    std::fs::rename(&crt_tmp, CERT_PATH).map_err(|e| format!("安装证书失败: {}", e))?;
    crate::sh::log(
        "info",
        &format!(
            "已生成 WebUI 证书（{} 天，SAN: {}）",
            CERT_DAYS,
            sans.join(",")
        ),
    );
    Ok(())
}

#[cfg(unix)]
fn set_mode(path: &str, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
}

#[cfg(not(unix))]
fn set_mode(_path: &str, _mode: u32) {}

#[cfg(unix)]
fn is_executable(path: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &str) -> bool {
    Path::new(path).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv6_literal_gets_brackets() {
        assert_eq!(format_host("::1"), "[::1]");
        assert_eq!(format_host("::"), "[::]");
        assert_eq!(format_host("0.0.0.0"), "0.0.0.0");
        assert_eq!(format_host("[::1]"), "[::1]");
    }

    #[test]
    fn internal_port_keeps_two_slots_apart() {
        assert_eq!(internal_port(4101), 5101);
        assert_eq!(internal_port(64535), 65535);
        assert_eq!(internal_port(65000), 64000);
    }

    #[test]
    fn sans_cover_host_and_loopback() {
        let sans = desired_sans("192.168.1.1");
        assert!(sans.contains(&"IP:192.168.1.1".to_string()));
        assert!(sans.contains(&"IP:127.0.0.1".to_string()));
        assert!(sans.contains(&"DNS:localhost".to_string()));

        let sans = desired_sans("router.lan");
        assert!(sans.contains(&"DNS:router.lan".to_string()));

        let sans = desired_sans("fd00::1");
        assert!(sans.contains(&"IP:fd00::1".to_string()));

        // 去重：回环地址不会重复出现
        let n = desired_sans("127.0.0.1")
            .iter()
            .filter(|s| s.as_str() == "IP:127.0.0.1")
            .count();
        assert_eq!(n, 1);
    }

    #[test]
    fn private_addr_filter() {
        assert!(is_private_addr("192.168.1.1"));
        assert!(is_private_addr("10.1.2.3"));
        assert!(is_private_addr("172.16.0.1"));
        assert!(is_private_addr("fd12::1"));

        assert!(!is_private_addr("100.64.0.1"));
        assert!(!is_private_addr("8.8.8.8"));
        assert!(!is_private_addr("fe80::1"));
        assert!(!is_private_addr("not-an-ip"));
    }

    #[test]
    fn random_password_shape() {
        let p = random_password(20).unwrap();
        assert_eq!(p.chars().count(), 20);
        assert!(p.chars().all(|c| c.is_ascii_alphanumeric()));
        assert!(!p.contains('l') && !p.contains('O') && !p.contains('0') && !p.contains('1'));
    }
}
