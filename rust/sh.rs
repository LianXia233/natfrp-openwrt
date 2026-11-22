//! 进程调用与 uci 读写的小封装。

use std::process::Command;

/// 执行命令，返回 (输出, 是否成功)。输出取 stdout，为空时退回 stderr，已 trim。
pub fn run(cmd: &str, args: &[&str]) -> (String, bool) {
    run_env(cmd, args, &[])
}

pub fn run_env(cmd: &str, args: &[&str], envs: &[(&str, &str)]) -> (String, bool) {
    let mut c = Command::new(cmd);
    c.args(args);
    for (k, v) in envs {
        c.env(k, v);
    }
    match c.output() {
        Ok(o) => {
            let mut s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if s.is_empty() {
                s = String::from_utf8_lossy(&o.stderr).trim().to_string();
            }
            (s, o.status.success())
        }
        Err(_) => (String::new(), false),
    }
}

/// 写 syslog。level 取 info / warning / err。
pub fn log(level: &str, msg: &str) {
    let pri = match level {
        "err" | "error" => "daemon.err",
        "warning" | "warn" => "daemon.warning",
        _ => "daemon.info",
    };
    let _ = run("logger", &["-t", "natfrpctl", "-p", pri, msg]);
}

pub const PKG: &str = "natfrp";

/// 读取 uci 值，不存在或空时返回空串
pub fn uci_get(name: &str) -> String {
    let path = format!("{}.{}", PKG, name);
    let (out, _) = run("uci", &["-q", "get", &path]);
    out
}

/// 一次性读出整份 `natfrp` 配置，返回 (键, 值) 列表，键形如 `main.webui_port`。
///
/// 页面每轮轮询都要取十来个键，逐个 `uci get` 每次 fork 一个进程，这里一次 `uci show`
/// 拿全量。解析失败时返回空列表，调用方会退回逐键读取。
pub fn uci_show() -> Vec<(String, String)> {
    let (out, ok) = run("uci", &["-q", "show", PKG]);
    if !ok || out.is_empty() {
        return Vec::new();
    }

    let mut map = Vec::new();
    for line in out.lines() {
        let line = line.trim();
        let eq = match line.find('=') {
            Some(i) => i,
            None => continue,
        };
        let (left, right) = line.split_at(eq);
        let key = match left.strip_prefix("natfrp.") {
            Some(k) => k,
            None => continue,
        };
        map.push((key.to_string(), unquote(&right[1..])));
    }
    map
}

/// uci 输出的值统一用单引号包住，内部的单引号写作 `'\''`
fn unquote(raw: &str) -> String {
    let v = raw.trim();
    if v.len() >= 2 && v.starts_with('\'') && v.ends_with('\'') {
        v[1..v.len() - 1].replace("'\\''", "'")
    } else {
        v.to_string()
    }
}

pub fn uci_delete(name: &str) {
    let path = format!("{}.{}", PKG, name);
    let _ = run("uci", &["-q", "delete", &path]);
}

pub fn uci_commit() {
    let _ = run("uci", &["commit", PKG]);
}

#[cfg(test)]
mod tests {
    use super::unquote;

    #[test]
    fn unquotes_plain_and_escaped() {
        assert_eq!(unquote("'4101'"), "4101");
        assert_eq!(unquote("'it'\\''s'"), "it's");
        assert_eq!(unquote("0.0.0.0"), "0.0.0.0");
    }
}
