//! 守护进程运行期配置 `/etc/natfrp/config.json` 的读写。
//!
//! 该文件由守护进程自己维护（含自动生成的字段），所以这里做「读出来、改几个键、写回去」，
//! 不重建整个文件，避免丢掉未知字段。

use crate::json::{self, J};
use std::fs;
use std::path::Path;

pub const DIR: &str = "/etc/natfrp";
pub const PATH: &str = "/etc/natfrp/config.json";

pub fn load() -> J {
    match fs::read_to_string(PATH) {
        Ok(text) => json::parse(&text).unwrap_or_else(|_| J::obj()),
        Err(_) => J::obj(),
    }
}

/// 原子写入（先写临时文件再 rename），并按 0600 收权限
pub fn save(doc: &J) -> Result<(), String> {
    let dir = Path::new(DIR);
    if !dir.exists() {
        fs::create_dir_all(dir).map_err(|e| format!("创建 {} 失败: {}", DIR, e))?;
    }
    let tmp = format!("{}.tmp", PATH);
    let body = doc.render() + "\n";
    fs::write(&tmp, body.as_bytes()).map_err(|e| format!("写入 {} 失败: {}", tmp, e))?;
    set_mode(&tmp, 0o600);
    fs::rename(&tmp, PATH).map_err(|e| format!("替换 {} 失败: {}", PATH, e))?;
    Ok(())
}

#[cfg(unix)]
fn set_mode(path: &str, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
}

#[cfg(not(unix))]
fn set_mode(_path: &str, _mode: u32) {}

pub fn set_str(doc: &mut J, key: &str, val: &str) {
    doc.set(key, J::Str(val.to_string()));
}

pub fn set_int(doc: &mut J, key: &str, val: i64) {
    doc.set(key, J::Num(val.to_string()));
}

pub fn set_bool(doc: &mut J, key: &str, val: bool) {
    doc.set(key, J::Bool(val));
}