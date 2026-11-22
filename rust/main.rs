//! natfrpctl — SakuraFrp 启动器的控制后端 / LuCI 页面的数据来源。
//!
//! 子命令：
//!   status   输出运行状态与 WebUI 信息（JSON）
//!   apply    将 uci 配置同步到守护进程配置并让服务生效
//!   render   仅同步配置文件（开机由 init 脚本调用）
//!   relay    明文 HTTP 入口（含 WebCrypto 兼容层）
//!   webui-cert  输出 WebUI 自签名证书（PEM），供浏览器导入信任
//!   version  输出本程序版本
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
const RELAY_OFFSET: i64 = 1000;
const DEFAULT_WEBUI_PORT: i64 = 4101;
const DEFAULT_WEBUI_HOST: &str = "0.0.0.0";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("help");

    let code = match cmd {
        "status" => cmd_status(),
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
                "用法: natfrpctl {status|apply|render|relay|webui-cert|version}
  status       运行状态与 WebUI 入口信息
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
    let _ = sh::run("logger", &["-t", "natfrpctl", "-p", "daemon.info", msg]);
}

fn ok_json(extra: Option<(&str, J)>) -> String {
    let mut out = J::obj();
    out.set("ok", J::Bool(true));
    if let Some((k, v)) = extra {
        out.set(k, v);
    }
    out.render()
}

/// 保存远程管理口令明文（仅 root 可读），供页面展示与复制
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

fn err_json(msg: &str) -> String {
    let mut out = J::obj();
    out.set("ok", J::Bool(false));
    out.set("error", J::Str(msg.to_string()));
    out.render()
}

// ---------------------------------------------------------------- status

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

fn service_version() -> String {
    let (out, ok) = sh::run_env(SERVICE_BIN, &["-V"], &[("NATFRP_SERVICE_WD", "/tmp")]);
    if !ok {
        return String::new();
    }
    out.split_whitespace().next().unwrap_or("").to_string()
}

fn cmd_status() -> i32 {
    let pid = pid_of();
    let cfg = config::load();
    let password = cfg.str_of("webui_pass").unwrap_or("").to_string();
    let token_set = cfg
        .str_of("token")
        .map(|t| !t.is_empty())
        .unwrap_or(false);

    // WebUI 对外暴露的地址取自 uci
    let webui_on = uci_bool("main.webui");
    let port = uci_int_or("main.webui_port", DEFAULT_WEBUI_PORT);
    let host = uci_str_or("main.webui_host", DEFAULT_WEBUI_HOST);

    let mut webui = J::obj();
    webui.set("enabled", J::Bool(webui_on && port > 0));
    webui.set("host", J::Str(host));
    webui.set("port", J::Num(port.to_string()));
    webui.set("tls", J::Bool(uci_tls_enabled()));
    let mut remote = J::obj();
    let remote_on = uci_bool("main.remote_mgmt");
    let remote_pass = load_remote_pass();
    remote.set("enabled", J::Bool(remote_on));
    remote.set("password", J::Str(remote_pass.clone()));
    remote.set(
        "keyed",
        J::Bool(
            cfg.str_of("remote_management_key")
                .map(|k| !k.is_empty())
                .unwrap_or(false),
        ),
    );

    webui.set("password", J::Str(password));

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
    out.set("version", J::Str(service_version()));
    out.set("token_set", J::Bool(token_set));
    out.set("webui", webui);
    out.set("remote", remote);

    print(&out.render());
    0
}

// ---------------------------------------------------------------- render

fn uci_bool(name: &str) -> bool {
    sh::uci_get(name) == "1"
}

fn uci_int_or(name: &str, def: i64) -> i64 {
    let raw = sh::uci_get(name);
    raw.parse::<i64>().unwrap_or(def)
}

fn uci_str_or(name: &str, def: &str) -> String {
    let raw = sh::uci_get(name);
    if raw.is_empty() {
        def.to_string()
    } else {
        raw
    }
}

/// WebUI 是否对外提供 HTTPS。关闭时守护进程只监听回环，
/// 由 `natfrpctl relay` 在对外端口提供明文入口。
fn uci_tls_enabled() -> bool {
    uci_str_or("main.webui_tls", "0") != "0"
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
    let mut uci_dirty = false;

    let token = sh::uci_get("main.token");
    let webui_on = uci_bool("main.webui");
    let check_update = uci_bool("main.check_update");
    let remote_on = uci_bool("main.remote_mgmt");
    let webui_pass = sh::uci_get("main.webui_pass");
    let remote_pass = sh::uci_get("main.remote_mgmt_pass");

    let mut port = uci_int_or("main.webui_port", DEFAULT_WEBUI_PORT);
    if port <= 0 || port > 65535 {
        port = DEFAULT_WEBUI_PORT;
    }
    let host = uci_str_or("main.webui_host", DEFAULT_WEBUI_HOST);

    let mut cfg = config::load();

    // 日志走 syslog，避免写 flash
    config::set_str(&mut cfg, "log_file", "");
    config::set_bool(&mut cfg, "log_stdout", true);

    if !token.is_empty() {
        config::set_str(&mut cfg, "token", &token);
        sh::uci_delete("main.token");
        uci_dirty = true;
    }

    if webui_on {
        if uci_tls_enabled() {
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
        }
        else if cfg
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

    // 开机自动启动的隧道：uci 里用逗号/空格分隔的隧道 ID
    let auto_tunnels = sh::uci_get("main.auto_start_tunnels");
    if !auto_tunnels.trim().is_empty() {
        let ids: Vec<J> = auto_tunnels
            .split(|c: char| c == ',' || c.is_whitespace())
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .map(|t| J::Num(t.to_string()))
            .collect();
        cfg.set("auto_start_tunnels", J::Arr(ids));
    }

    config::set_bool(&mut cfg, "remote_management", remote_on);
    let has_key = cfg
        .str_of("remote_management_key")
        .map(|k| !k.is_empty())
        .unwrap_or(false);

    if remote_on && !remote_pass.is_empty() {
        let pass = remote_pass.as_str();
        let (key, ok) = sh::run_env(SERVICE_BIN, &["remote-kdf", pass], &[("NATFRP_SERVICE_WD", config::DIR)]);
        if ok && !key.is_empty() {
            config::set_str(&mut cfg, "remote_management_key", &key);
            sh::uci_delete("main.remote_mgmt_pass");
            uci_dirty = true;
            let _ = save_remote_pass(pass);
        } else {
            return Err("E2E 密码转换失败，请检查密码内容".to_string());
        }
    }
    else if remote_on && !has_key {
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

// ---------------------------------------------------------------- apply

fn cmd_apply() -> i32 {
    let before = std::fs::read_to_string(config::PATH).unwrap_or_default();
    let uci_dirty = match render_config() {
        Ok(d) => d,
        Err(e) => {
            note(&format!("apply aborted: {}", e));
            print(&err_json(&e));
            return 1;
        }
    };
    let after = std::fs::read_to_string(config::PATH).unwrap_or_default();
    let cfg_changed = before != after;

    let enabled = uci_bool("main.enabled");
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
    if uci_tls_enabled() {
        note("relay: TLS 已启用，无需明文入口");
        return 0;
    }
    if !uci_bool("main.webui") {
        note("relay: WebUI 未启用");
        return 0;
    }

    let host = uci_str_or("main.webui_host", DEFAULT_WEBUI_HOST);
    let port = uci_int_or("main.webui_port", DEFAULT_WEBUI_PORT);
    if port <= 0 || port > 65535 {
        note("relay: 端口无效");
        return 1;
    }

    let bind = format!("{}:{}", host, port);
    let upstream = format!("127.0.0.1:{}", internal_port(port));

    match relay::serve(&bind, &upstream) {
        Ok(()) => 0,
        Err(e) => {
            note(&format!("relay: {}", e));
            1
        }
    }
}

// ---------------------------------------------------------------- cert

/// 输出 WebUI 使用的自签名证书（PEM），供用户导入浏览器信任库。
fn cmd_webui_cert() -> i32 {
    let cfg = config::load();
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
