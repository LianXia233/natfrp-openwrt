//! 守护进程运行期配置 `/etc/natfrp/config.json` 的读写。
//!
//! 该文件由守护进程自己维护（含自动生成的字段），所以这里做「读出来、改几个键、写回去」，
//! 不重建整个文件，避免丢掉未知字段。
//!
//! 读取语义很重要：解析失败**绝不能**当成空对象继续。文件里的 token 与口令是唯一副本
//! （uci 里的对应项在落盘后已被删除），一旦静默重写成空配置，用户就会在没有提示的情况下
//! 掉线、旧口令失效。因此这里区分三种情况：缺文件（全新安装）、解析成功、主文件损坏但
//! 可从 `.bak` 恢复；三者都不成立时返回错误，由调用方放弃写入、保留原文件。

use crate::json::{self, J};
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

pub const DIR: &str = "/etc/natfrp";
pub const PATH: &str = "/etc/natfrp/config.json";
#[cfg_attr(not(test), allow(dead_code))]
pub const BAK_PATH: &str = "/etc/natfrp/config.json.bak";

/// 备份文件路径：`<主文件>.bak`。守护进程自己也用这个后缀，互相兼容。
fn bak_of(path: &str) -> String {
    format!("{}.bak", path)
}

pub fn load() -> Result<J, String> {
    load_from(PATH)
}

pub fn load_from(path: &str) -> Result<J, String> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        // 文件不存在＝全新安装，允许从空配置起步
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(J::obj()),
        Err(e) => return Err(format!("读取 {} 失败: {}", path, e)),
    };

    // 空文件本身就是掉电截断的常见形态，按损坏处理
    if text.trim().is_empty() {
        return recover(path, "文件为空，可能是写入过程中断");
    }

    match json::parse(&text) {
        Ok(doc) => Ok(doc),
        Err(e) => recover(path, &e),
    }
}

/// 主文件损坏时尝试 `.bak`；再失败就放弃，绝不返回空对象
fn recover(path: &str, why: &str) -> Result<J, String> {
    let bak = bak_of(path);
    if let Ok(text) = fs::read_to_string(&bak) {
        if let Ok(doc) = json::parse(&text) {
            crate::sh::log(
                "warning",
                &format!(
                    "{} 解析失败（{}），已改从 {} 恢复；损坏的原文件未改动",
                    path, why, bak
                ),
            );
            return Ok(doc);
        }
    }

    crate::sh::log(
        "err",
        &format!(
            "{} 解析失败（{}），且没有可用的备份；本次不写入，保留原文件待人工处理",
            path, why
        ),
    );
    Err(format!(
        "{} 解析失败（{}）。为避免覆盖现有密钥，本次已放弃写入；\
         请检查该文件（或删除后重新保存一次以重新生成配置）",
        path, why
    ))
}

/// 原子写入（先写临时文件再 rename），并按 0600 收权限。
/// 覆盖前先留一份 `.bak`，供下次损坏时恢复。
pub fn save(doc: &J) -> Result<(), String> {
    save_to(PATH, doc)
}

pub fn save_to(path: &str, doc: &J) -> Result<(), String> {
    let dir = match Path::new(path).parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => Path::new(DIR).to_path_buf(),
    };
    if !dir.exists() {
        fs::create_dir_all(&dir).map_err(|e| format!("创建 {} 失败: {}", dir.display(), e))?;
    }

    // 备份：只在现有文件可解析时留，避免把坏内容固化成「好备份」
    if let Ok(text) = fs::read_to_string(path) {
        if json::parse(&text).is_ok() {
            let _ = fs::copy(path, bak_of(path));
        }
    }

    let tmp = format!("{}.tmp", path);
    let body = doc.render() + "\n";
    fs::write(&tmp, body.as_bytes()).map_err(|e| format!("写入 {} 失败: {}", tmp, e))?;
    set_mode(&tmp, 0o600);
    fs::rename(&tmp, path).map_err(|e| format!("替换 {} 失败: {}", path, e))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let mut d = std::env::temp_dir();
        d.push(format!("natfrpcfg-{}-{}", tag, std::process::id()));
        let _ = fs::create_dir_all(&d);
        d
    }

    #[test]
    fn missing_file_is_empty_config() {
        let d = tmpdir("missing");
        let p = d.join("config.json");
        assert_eq!(load_from(p.to_str().unwrap()).unwrap(), J::obj());
    }

    #[test]
    fn corrupted_file_does_not_become_empty_config() {
        let d = tmpdir("corrupt");
        let p = d.join("config.json");
        fs::write(&p, b"{\"token\":\"SECRET\",\"webui_po").unwrap();
        assert!(load_from(p.to_str().unwrap()).is_err());
    }

    #[test]
    fn corrupted_file_recovers_from_backup() {
        let d = tmpdir("bak");
        let p = d.join("config.json");
        let text = r#"{"token":"SECRET","webui_port":4101}"#;
        fs::write(format!("{}.bak", p.display()), text).unwrap();
        fs::write(&p, b"{\"token\":\"SECRET\",\"webui_po").unwrap();

        let doc = load_from(p.to_str().unwrap()).unwrap();
        assert_eq!(doc.str_of("token"), Some("SECRET"));
    }

    #[test]
    fn save_keeps_a_parsable_backup() {
        let d = tmpdir("save");
        let p = d.join("config.json");
        save_to(p.to_str().unwrap(), &json::parse(r#"{"token":"T1"}"#).unwrap()).unwrap();
        save_to(p.to_str().unwrap(), &json::parse(r#"{"token":"T2"}"#).unwrap()).unwrap();

        let bak = fs::read_to_string(format!("{}.bak", p.display())).unwrap();
        assert_eq!(json::parse(&bak).unwrap().str_of("token"), Some("T1"));
        let cur = fs::read_to_string(&p).unwrap();
        assert_eq!(json::parse(&cur).unwrap().str_of("token"), Some("T2"));
    }

    #[test]
    fn save_never_backs_up_broken_content() {
        let d = tmpdir("badbak");
        let p = d.join("config.json");
        let mut f = fs::File::create(&p).unwrap();
        write!(f, "{{broken").unwrap();
        drop(f);

        save_to(p.to_str().unwrap(), &json::parse(r#"{"token":"T"}"#).unwrap()).unwrap();
        assert!(!Path::new(&format!("{}.bak", p.display())).exists());
    }
}
