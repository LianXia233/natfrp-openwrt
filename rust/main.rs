//! natfrpctl — SakuraFrp 启动器的控制后端 / LuCI 页面的数据来源。
//!
//! 子命令：
//!   status     运行状态（JSON，不含口令；只读账号也可执行）
//!   secrets    WebUI / 远程管理口令（需要写权限）
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
const DEFAULT_CERT_PATH: &str = "/etc/natfrp/webui.crt";
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
        "apply" => cmd_apply(),
        "render" => cmd_render_only(),
        "relay" => cmd_relay(),
        "webui-cert" => cmd_webui_cert(),
        "version" | "-V" | "--version" => {
            print(&ok_json(Some(("version", J::Str(env!("CARGO_PKG_VERSION").into())))));
            0
        }
        _ => {
            print(
                "用法: natfrpctl {status|secrets|apply|render|relay|webui-cert|version}
  status       运行状态与 WebUI 入口信息
  secrets      口令（WebUI 连接密码 / 远程管理密码）
  apply        应用 uci 配置并重启服务
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
        Uci { map: sh::uci_show() }
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

/// 生成随机口令（去掉易混淆字符 l/1/O/0）
fn random_password(len: usize) -> Result<String, String> {
    use std::io::Read;

    const ALPHABET: &[u8] = b"abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";

    let mut buf = vec![0u8; len];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut buf))
        .map_err(|e| format!("读取随机数失败: {}", e))?;

    Ok(buf
        .iter()
        .map(|b| ALPHABET[*b as usize % ALPHABET.len()] as char)
        .collect())
}

fn save_remote_pass(pass: &str) -> Result<(), String> {
    let body = format!("{}\n", pass);
    std::fs::write(REMOTE_PASS_FILE, body.as_bytes())
        .map_err(|e| format!("写入 {} 失败: {}", REMOTE_PASS_FILE, e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(
            REMOTE_PASS_FILE,
            std::fs::Permissions::from_mode(0o600),
        );
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
    let _ = std::fs::write(VERSION_CACHE, format!("{}\n{}\n", stamp, version).as_bytes());
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
    let uci = Uci::new();
    let pid = pid_of();

    let (cfg, cfg_error) = match config::load() {
        Ok(c) => (c, None::<String>),
        Err(e) => (J::obj(), Some(e)),
    };

    let token_set = cfg
        .str_of("token")
        .map(|t| !t.is_empty())
        .unwrap_or(false);

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
        config::set_int(&mut cfg, "update_interval", if check_update { 604800 } else { -1 });
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

fn cmd_apply() -> i32 {
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
    if !enabled {
        if running {
            let (_, ok) = sh::run(INITD, &["stop"]);
            note(&format!("service stop -> {}", ok));
        }
    } else if !running {
        let (_, ok) = sh::run(INITD, &["start"]);
        note(&format!("service start -> {}", ok));
    } else if cfg_changed || uci_dirty {
        // 配置变了才重启，避免每次保存都断流
        let (_, ok) = sh::run(INITD, &["reload"]);
        note(&format!("service reload -> {}", ok));
    }

    cmd_status()
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
            &format!("relay: 端口 {} 不可用（关闭 TLS 时需大于 {}）", port, RELAY_OFFSET),
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
        .unwrap_or_else(|| DEFAULT_CERT_PATH.to_string());

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
}
