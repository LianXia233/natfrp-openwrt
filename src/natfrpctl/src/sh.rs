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

pub const PKG: &str = "natfrp";

/// 读取 uci 值，不存在或空时返回空串
pub fn uci_get(name: &str) -> String {
    let path = format!("{}.{}", PKG, name);
    let (out, _) = run("uci", &["-q", "get", &path]);
    out
}

pub fn uci_delete(name: &str) {
    let path = format!("{}.{}", PKG, name);
    let _ = run("uci", &["-q", "delete", &path]);
}

pub fn uci_commit() {
    let _ = run("uci", &["commit", PKG]);
}