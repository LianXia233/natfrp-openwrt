//! 明文 HTTP 入口。
//!
//! 守护进程的 WebUI 只提供 HTTPS，且登录依赖浏览器的 WebCrypto（仅安全上下文可用）。
//! 这里在对外端口上做两件事：
//!   1. 明文 HTTP 接入，转发给回环上的 HTTPS 端口；
//!   2. 给首页 HTML 注入一个 WebCrypto 兼容层，让登录在 http:// 下也能完成。
//!
//! WebSocket 升级后切为裸字节转发，gRPC 流量不受影响。

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::thread;

const OPENSSL_BIN: &str = "/usr/bin/openssl";
const INJECT_JS: &str = include_str!("inject.js");
const MAX_HEAD: usize = 64 * 1024;

pub fn serve(bind: &str, upstream: &str) -> Result<(), String> {
    let listener =
        TcpListener::bind(bind).map_err(|e| format!("监听 {} 失败: {}", bind, e))?;

    for conn in listener.incoming() {
        if let Ok(stream) = conn {
            let up = upstream.to_string();
            thread::spawn(move || handle(stream, &up));
        }
    }

    Ok(())
}

fn handle(mut client: TcpStream, upstream: &str) {
    let mut child = match Command::new(OPENSSL_BIN)
        .args([
            "s_client",
            "-quiet",
            "-connect",
            upstream,
            "-servername",
            "localhost",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return,
    };

    let mut to_up = match child.stdin.take() {
        Some(s) => s,
        None => {
            let _ = child.kill();
            return;
        }
    };
    let mut from_up = match child.stdout.take() {
        Some(s) => s,
        None => {
            let _ = child.kill();
            return;
        }
    };

    let mut req = Vec::new();
    if !read_head(&mut client, &mut req) {
        let _ = child.kill();
        let _ = child.wait();
        return;
    }

    let lower = String::from_utf8_lossy(&req).to_lowercase();

    if lower.contains("upgrade: websocket") {
        relay_websocket(client, req, to_up, from_up, child);
        return;
    }

    let with_close = force_connection_close(&req);
    if to_up.write_all(&with_close).is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return;
    }
    let _ = to_up.flush();

    let mut resp = Vec::new();
    let _ = from_up.read_to_end(&mut resp);

    if !resp.is_empty() {
        let out = rewrite_response(&resp);
        let _ = client.write_all(&out);
        let _ = client.flush();
    }

    let _ = child.kill();
    let _ = child.wait();
}

fn relay_websocket(
    mut client: TcpStream,
    req: Vec<u8>,
    mut to_up: std::process::ChildStdin,
    mut from_up: std::process::ChildStdout,
    mut child: Child,
) {
    if to_up.write_all(&req).is_err() {
        let _ = child.kill();
        return;
    }
    let _ = to_up.flush();

    let reader = match client.try_clone() {
        Ok(c) => c,
        Err(_) => {
            let _ = child.kill();
            return;
        }
    };

    let up_thread = thread::spawn(move || {
        let mut sock = reader;
        let mut buf = [0u8; 8192];
        loop {
            match sock.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if to_up.write_all(&buf[..n]).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = to_up.flush();
    });

    let mut buf = [0u8; 8192];
    loop {
        match from_up.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if client.write_all(&buf[..n]).is_err() {
                    break;
                }
            }
            Err(_) => break,
        }
    }

    let _ = client.flush();
    let _ = child.kill();
    let _ = child.wait();
    let _ = up_thread.join();
}

/// 读到请求头结束（含空行）
fn read_head(sock: &mut TcpStream, out: &mut Vec<u8>) -> bool {
    let mut buf = [0u8; 4096];
    loop {
        if out.windows(4).any(|w| w == b"\r\n\r\n") {
            return true;
        }
        if out.len() > MAX_HEAD {
            return false;
        }
        match sock.read(&mut buf) {
            Ok(0) => return !out.is_empty(),
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(_) => return false,
        }
    }
}

fn force_connection_close(req: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(req);
    let mut idx = match text.find("\r\n\r\n") {
        Some(i) => i,
        None => return req.to_vec(),
    };
    let head = &text[..idx];

    let mut lines: Vec<String> = Vec::new();
    for line in head.split("\r\n") {
        if line.to_lowercase().starts_with("connection:") {
            continue;
        }
        lines.push(line.to_string());
    }
    lines.push("Connection: close".to_string());

    let mut out = lines.join("\r\n");
    out.push_str("\r\n\r\n");

    // 原请求若带正文（POST 等），一并跟上
    idx += 4;
    let body = &req[idx.min(req.len())..];
    let mut bytes = out.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

fn rewrite_response(resp: &[u8]) -> Vec<u8> {
    let pos = match find(resp, b"\r\n\r\n") {
        Some(p) => p,
        None => return resp.to_vec(),
    };
    let head = String::from_utf8_lossy(&resp[..pos]).to_string();
    let body = &resp[pos + 4..];

    let mut status_ok = false;
    let mut content_type = String::new();
    let mut chunked = false;
    let mut headers: Vec<String> = Vec::new();

    for (i, line) in head.split("\r\n").enumerate() {
        let low = line.to_lowercase();
        if i == 0 {
            status_ok = low.contains(" 200 ");
        } else if low.starts_with("content-type:") {
            content_type = low.clone();
        } else if low.starts_with("transfer-encoding:") && low.contains("chunked") {
            chunked = true;
        } else if low.starts_with("content-length:")
            || low.starts_with("connection:")
            || low.starts_with("transfer-encoding:")
        {
            continue;
        }
        headers.push(line.to_string());
    }

    let is_html = content_type.contains("text/html");

    let mut final_body: Vec<u8> = if is_html {
        let raw = if chunked { dechunk(body) } else { body.to_vec() };
        inject(&raw)
    } else {
        body.to_vec()
    };

    if !is_html && chunked {
        // 非 HTML 且分块编码：保持原样交给浏览器
        let mut out = head.into_bytes();
        out.extend_from_slice(b"\r\n\r\n");
        out.extend_from_slice(body);
        return out;
    }

    let mut out = String::new();
    out.push_str(&headers[0]);
    out.push_str("\r\n");
    for line in headers.iter().skip(1) {
        out.push_str(line);
        out.push_str("\r\n");
    }
    if status_ok {
        out.push_str(&format!("Content-Length: {}\r\n", final_body.len()));
    }
    out.push_str("Connection: close\r\n\r\n");

    let mut bytes = out.into_bytes();
    bytes.append(&mut final_body);
    bytes
}

fn inject(html: &[u8]) -> Vec<u8> {
    let snippet = format!("<script>{}</script>", INJECT_JS);
    let tag = snippet.as_bytes();

    // 尽量插在 <head> 之后，保证早于页面自己的模块脚本执行
    if let Some(h) = find_ci(html, b"<head") {
        if let Some(gt) = html[h..].iter().position(|b| *b == b'>') {
            let at = h + gt + 1;
            let mut out = Vec::with_capacity(html.len() + tag.len());
            out.extend_from_slice(&html[..at]);
            out.extend_from_slice(tag);
            out.extend_from_slice(&html[at..]);
            return out;
        }
    }

    let mut out = Vec::with_capacity(html.len() + tag.len());
    out.extend_from_slice(tag);
    out.extend_from_slice(html);
    out
}

fn dechunk(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < body.len() {
        let line_end = match find(&body[i..], b"\r\n") {
            Some(p) => i + p,
            None => break,
        };
        let size_str = String::from_utf8_lossy(&body[i..line_end]);
        let size = match usize::from_str_radix(size_str.trim().split(';').next().unwrap_or("0").trim(), 16)
        {
            Ok(s) => s,
            Err(_) => break,
        };
        i = line_end + 2;
        if size == 0 {
            break;
        }
        let end = (i + size).min(body.len());
        out.extend_from_slice(&body[i..end]);
        i = end + 2;
    }
    out
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

fn find_ci(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| {
        w.iter()
            .zip(needle.iter())
            .all(|(a, b)| a.to_ascii_lowercase() == b.to_ascii_lowercase())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injects_after_head() {
        let html = b"<html><head><title>x</title></head><body>hi</body></html>";
        let out = inject(html);
        let text = String::from_utf8_lossy(&out);
        assert!(text.starts_with("<html><head><script>"));
        assert!(text.contains("<title>x</title>"));
        assert!(text.ends_with("</html>"));
    }

    #[test]
    fn decodes_chunked() {
        let body = b"5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
        assert_eq!(dechunk(body), b"hello world".to_vec());
    }

    #[test]
    fn rewrites_html_with_length() {
        let resp = b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 10\r\n\r\n<html></html>";
        let out = rewrite_response(resp);
        let text = String::from_utf8_lossy(&out);
        assert!(text.contains("Connection: close"));
        assert!(text.contains(&format!("Content-Length: {}", out.len() - text.find("\r\n\r\n").unwrap() - 4)));
        assert!(text.contains("<script>"));
    }
}
