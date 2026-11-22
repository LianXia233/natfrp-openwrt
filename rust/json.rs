//! 极简 JSON 读写。
//!
//! 只处理本程序自己的场景：读取 `config.json`（对象、有序键）、输出状态 JSON。
//! 因此不引入 serde，也不追求规范里的边角情况（如非 BMP 的 \u 代理对）。

#[derive(Clone, Debug, PartialEq)]
pub enum J {
    Null,
    Bool(bool),
    /// 保留原始字面量，避免整数被浮点化
    Num(String),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    pub fn obj() -> J {
        J::Obj(Vec::new())
    }

    pub fn get(&self, key: &str) -> Option<&J> {
        match self {
            J::Obj(kv) => kv.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn str_of(&self, key: &str) -> Option<&str> {
        match self.get(key) {
            Some(J::Str(s)) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn i64_of(&self, key: &str) -> Option<i64> {
        match self.get(key) {
            Some(J::Num(n)) => n.parse().ok(),
            _ => None,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn bool_of(&self, key: &str) -> Option<bool> {
        match self.get(key) {
            Some(J::Bool(b)) => Some(*b),
            _ => None,
        }
    }

    /// 覆盖已有键（保持原位置），或追加到末尾
    pub fn set(&mut self, key: &str, val: J) {
        if let J::Obj(kv) = self {
            if let Some(slot) = kv.iter_mut().find(|(k, _)| k == key) {
                slot.1 = val;
            } else {
                kv.push((key.to_string(), val));
            }
        }
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        self.emit(&mut out);
        out
    }

    fn emit(&self, out: &mut String) {
        match self {
            J::Null => out.push_str("null"),
            J::Bool(true) => out.push_str("true"),
            J::Bool(false) => out.push_str("false"),
            J::Num(n) => out.push_str(n),
            J::Str(s) => emit_str(s, out),
            J::Arr(items) => {
                out.push('[');
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.emit(out);
                }
                out.push(']');
            }
            J::Obj(kv) => {
                out.push('{');
                for (i, (k, v)) in kv.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    emit_str(k, out);
                    out.push(':');
                    v.emit(out);
                }
                out.push('}');
            }
        }
    }
}

fn emit_str(s: &str, out: &mut String) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

pub fn parse(text: &str) -> Result<J, String> {
    let mut p = P { b: text.as_bytes(), i: 0 };
    p.skip_ws();
    let v = p.value()?;
    p.skip_ws();
    if p.i != p.b.len() {
        return Err(format!("trailing data at byte {}", p.i));
    }
    Ok(v)
}

struct P<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> P<'a> {
    fn skip_ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn lit(&mut self, word: &str) -> Result<(), String> {
        if self.b[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            Ok(())
        } else {
            Err(format!("expected `{}` at byte {}", word, self.i))
        }
    }

    fn value(&mut self) -> Result<J, String> {
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => Ok(J::Str(self.string()?)),
            Some(b't') => {
                self.lit("true")?;
                Ok(J::Bool(true))
            }
            Some(b'f') => {
                self.lit("false")?;
                Ok(J::Bool(false))
            }
            Some(b'n') => {
                self.lit("null")?;
                Ok(J::Null)
            }
            Some(_) => self.number(),
            None => Err("unexpected end of input".to_string()),
        }
    }

    fn object(&mut self) -> Result<J, String> {
        self.i += 1; // '{'
        let mut kv = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Ok(J::Obj(kv));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            if self.peek() != Some(b':') {
                return Err(format!("expected `:` at byte {}", self.i));
            }
            self.i += 1;
            self.skip_ws();
            let val = self.value()?;
            kv.push((key, val));
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.i += 1;
                }
                Some(b'}') => {
                    self.i += 1;
                    return Ok(J::Obj(kv));
                }
                _ => return Err(format!("expected `,` or `}}` at byte {}", self.i)),
            }
        }
    }

    fn array(&mut self) -> Result<J, String> {
        self.i += 1; // '['
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok(J::Arr(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value()?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.i += 1;
                }
                Some(b']') => {
                    self.i += 1;
                    return Ok(J::Arr(items));
                }
                _ => return Err(format!("expected `,` or `]` at byte {}", self.i)),
            }
        }
    }

    fn string(&mut self) -> Result<String, String> {
        if self.peek() != Some(b'"') {
            return Err(format!("expected string at byte {}", self.i));
        }
        self.i += 1;
        let mut out = String::new();
        loop {
            let c = self.peek().ok_or_else(|| "unterminated string".to_string())?;
            self.i += 1;
            match c {
                b'"' => return Ok(out),
                b'\\' => {
                    let e = self.peek().ok_or_else(|| "bad escape".to_string())?;
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let mut code: u32 = 0;
                            for _ in 0..4 {
                                let h = self.peek().ok_or_else(|| "bad \\u".to_string())?;
                                self.i += 1;
                                let d = (h as char)
                                    .to_digit(16)
                                    .ok_or_else(|| "bad \\u digit".to_string())?;
                                code = code * 16 + d;
                            }
                            out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                        }
                        _ => return Err("unknown escape".to_string()),
                    }
                }
                _ => {
                    // 逐字节读取 UTF-8 序列
                    let start = self.i - 1;
                    while self.i < self.b.len() && (self.b[self.i] & 0xC0) == 0x80 {
                        self.i += 1;
                    }
                    out.push_str(&String::from_utf8_lossy(&self.b[start..self.i]));
                }
            }
        }
    }

    fn number(&mut self) -> Result<J, String> {
        let start = self.i;
        while let Some(c) = self.peek() {
            if matches!(c, b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E') {
                self.i += 1;
            } else {
                break;
            }
        }
        if start == self.i {
            return Err(format!("unexpected byte at {}", self.i));
        }
        let raw = std::str::from_utf8(&self.b[start..self.i])
            .map_err(|_| "invalid number".to_string())?;
        Ok(J::Num(raw.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_config_like() {
        let src = r#"{"version":"3.1.9","token":"","update_interval":-1,"remote_management":true,"webui_pass":"a\"b"}"#;
        let v = parse(src).expect("parse");
        assert_eq!(v.str_of("version"), Some("3.1.9"));
        assert_eq!(v.i64_of("update_interval"), Some(-1));
        assert_eq!(v.bool_of("remote_management"), Some(true));
        assert_eq!(v.str_of("webui_pass"), Some("a\"b"));
        // 重新解析自己的输出
        let re = parse(&v.render()).expect("reparse");
        assert_eq!(re, v);
    }

    #[test]
    fn set_keeps_position_and_appends() {
        let mut v = parse(r#"{"a":1,"b":2}"#).unwrap();
        v.set("a", J::Num("9".into()));
        v.set("c", J::Bool(true));
        assert_eq!(v.render(), r#"{"a":9,"b":2,"c":true}"#);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("{").is_err());
        assert!(parse(r#"{"a":}"#).is_err());
        assert!(parse(r#"{"a":1} x"#).is_err());
    }
}