//! Just enough JSON for the journal.
//!
//! Std-only by policy (`N-11`). Numbers are kept as their literal text rather
//! than parsed into a float: the journal only ever writes integers, and keeping
//! the literal means a record written by a future version — with a field this
//! version has never heard of, in a shape it does not use — still reads back
//! unchanged (`N-8`).

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    /// The numeric literal, verbatim.
    Num(String),
    Str(String),
    Arr(Vec<Value>),
    /// Insertion-ordered, so a record round-trips byte-for-byte.
    Obj(Vec<(String, Value)>),
}

impl Value {
    pub fn int(n: i64) -> Value {
        Value::Num(n.to_string())
    }

    pub fn str(s: impl Into<String>) -> Value {
        Value::Str(s.into())
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Obj(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Num(n) => n.parse().ok(),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_arr(&self) -> Option<&[Value]> {
        match self {
            Value::Arr(items) => Some(items),
            _ => None,
        }
    }
}

pub fn to_string(value: &Value) -> String {
    let mut out = String::new();
    write_value(value, &mut out);
    out
}

fn write_value(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Num(n) => out.push_str(n),
        Value::Str(s) => write_string(s, out),
        Value::Arr(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(item, out);
            }
            out.push(']');
        }
        Value::Obj(pairs) => {
            out.push('{');
            for (i, (key, val)) in pairs.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(key, out);
                out.push(':');
                write_value(val, out);
            }
            out.push('}');
        }
    }
}

fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // A raw control character in a journal line would make the file
            // unreadable by anything else; escape the whole range.
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

pub fn parse(input: &str) -> Result<Value> {
    let mut p = Parser { bytes: input.as_bytes(), pos: 0 };
    p.skip_ws();
    let value = p.value()?;
    p.skip_ws();
    if p.pos != p.bytes.len() {
        return Err(Error::Json { at: p.pos, reason: "trailing content".into() });
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn err<T>(&self, reason: &str) -> Result<T> {
        Err(Error::Json { at: self.pos, reason: reason.into() })
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn literal(&mut self, word: &str) -> Result<()> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(())
        } else {
            self.err(&format!("expected `{word}`"))
        }
    }

    fn value(&mut self) -> Result<Value> {
        match self.peek() {
            None => self.err("unexpected end of input"),
            Some(b'n') => self.literal("null").map(|()| Value::Null),
            Some(b't') => self.literal("true").map(|()| Value::Bool(true)),
            Some(b'f') => self.literal("false").map(|()| Value::Bool(false)),
            Some(b'"') => self.string().map(Value::Str),
            Some(b'[') => self.array(),
            Some(b'{') => self.object(),
            Some(c) if c == b'-' || c.is_ascii_digit() => self.number(),
            Some(c) => self.err(&format!("unexpected byte `{}`", c as char)),
        }
    }

    fn number(&mut self) -> Result<Value> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        while matches!(self.peek(), Some(c) if c.is_ascii_digit() || matches!(c, b'.' | b'e' | b'E' | b'+' | b'-'))
        {
            self.pos += 1;
        }
        if self.pos == start {
            return self.err("empty number");
        }
        match std::str::from_utf8(&self.bytes[start..self.pos]) {
            Ok(text) => Ok(Value::Num(text.to_string())),
            Err(_) => self.err("number is not valid UTF-8"),
        }
    }

    fn array(&mut self) -> Result<Value> {
        self.pos += 1; // [
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Value::Arr(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value()?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Value::Arr(items));
                }
                _ => return self.err("expected `,` or `]`"),
            }
        }
    }

    fn object(&mut self) -> Result<Value> {
        self.pos += 1; // {
        let mut pairs = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Value::Obj(pairs));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            if self.peek() != Some(b':') {
                return self.err("expected `:`");
            }
            self.pos += 1;
            self.skip_ws();
            let value = self.value()?;
            pairs.push((key, value));
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Value::Obj(pairs));
                }
                _ => return self.err("expected `,` or `}`"),
            }
        }
    }

    fn string(&mut self) -> Result<String> {
        if self.peek() != Some(b'"') {
            return self.err("expected a string");
        }
        self.pos += 1;
        let mut out = Vec::new();
        loop {
            let byte = match self.peek() {
                Some(b) => b,
                None => return self.err("unterminated string"),
            };
            self.pos += 1;
            match byte {
                b'"' => {
                    return match String::from_utf8(out) {
                        Ok(s) => Ok(s),
                        Err(_) => self.err("string is not valid UTF-8"),
                    }
                }
                b'\\' => {
                    let esc = match self.peek() {
                        Some(b) => b,
                        None => return self.err("unterminated escape"),
                    };
                    self.pos += 1;
                    match esc {
                        b'"' => out.push(b'"'),
                        b'\\' => out.push(b'\\'),
                        b'/' => out.push(b'/'),
                        b'b' => out.push(0x08),
                        b'f' => out.push(0x0c),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'u' => {
                            let ch = self.unicode_escape()?;
                            let mut buf = [0u8; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        other => return self.err(&format!("unknown escape `\\{}`", other as char)),
                    }
                }
                other => out.push(other),
            }
        }
    }

    /// A `\u` escape, including the surrogate pair a character outside the BMP
    /// is written as.
    fn unicode_escape(&mut self) -> Result<char> {
        let first = self.hex4()?;
        let code = if (0xD800..0xDC00).contains(&first) {
            if !self.bytes[self.pos..].starts_with(b"\\u") {
                return self.err("high surrogate without a low surrogate");
            }
            self.pos += 2;
            let second = self.hex4()?;
            if !(0xDC00..0xE000).contains(&second) {
                return self.err("expected a low surrogate");
            }
            0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
        } else {
            first
        };
        match char::from_u32(code) {
            Some(ch) => Ok(ch),
            None => self.err("escape is not a character"),
        }
    }

    fn hex4(&mut self) -> Result<u32> {
        if self.pos + 4 > self.bytes.len() {
            return self.err("truncated escape");
        }
        let digits = match std::str::from_utf8(&self.bytes[self.pos..self.pos + 4]) {
            Ok(d) => d,
            Err(_) => return self.err("escape is not valid UTF-8"),
        };
        match u32::from_str_radix(digits, 16) {
            Ok(code) => {
                self.pos += 4;
                Ok(code)
            }
            Err(_) => self.err("escape is not hexadecimal"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(pairs: &[(&str, Value)]) -> Value {
        Value::Obj(pairs.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect())
    }

    #[test]
    fn writes_and_reads_a_record_shaped_object() {
        let value = obj(&[
            ("v", Value::int(1)),
            ("step", Value::str("c1/b1/s03")),
            ("ok", Value::Bool(true)),
            ("requirements", Value::Arr(vec![Value::str("L-3"), Value::str("L-4")])),
            ("detail", Value::Null),
        ]);
        let text = to_string(&value);
        assert_eq!(
            text,
            r#"{"v":1,"step":"c1/b1/s03","ok":true,"requirements":["L-3","L-4"],"detail":null}"#
        );
        assert_eq!(parse(&text).expect("round trip"), value);
    }

    #[test]
    fn escapes_what_would_break_a_line() {
        // A journal line holding verbatim error text (`L-16`) will contain
        // newlines and quotes; one raw newline would split the record in two.
        let value = Value::str("error: expected \"x\"\n\tat line 3\\col 2");
        let text = to_string(&value);
        assert!(!text.contains('\n'), "no raw newline may survive: {text}");
        assert_eq!(parse(&text).expect("round trip"), value);
    }

    #[test]
    fn escapes_control_characters_as_hex() {
        let escaped = to_string(&Value::str("a\u{1}b"));
        assert!(escaped.contains("u0001"), "control char must be hex-escaped: {escaped}");
        assert_eq!(parse(&escaped).expect("round trip"), Value::str("a\u{1}b"));
    }

    #[test]
    fn reads_a_surrogate_pair() {
        assert_eq!(parse(r#""😀""#).expect("parse"), Value::str("😀"));
    }

    #[test]
    fn keeps_unknown_fields_and_shapes() {
        // `N-8`: a record written by a later version must survive a round trip
        // through this one, including a float it has no field for.
        let text = r#"{"v":2,"weird":{"nested":[1.5,-2e3]},"extra":"kept"}"#;
        let value = parse(text).expect("parse");
        assert_eq!(to_string(&value), text);
    }

    #[test]
    fn rejects_malformed_input() {
        for bad in [r#"{"a":}"#, r#"{"a" 1}"#, "[1,", r#""unterminated"#, "{} junk"] {
            assert!(parse(bad).is_err(), "should have rejected: {bad}");
        }
    }

    #[test]
    fn reads_utf8_outside_ascii() {
        let value = Value::str("naïve — 日本語");
        assert_eq!(parse(&to_string(&value)).expect("round trip"), value);
    }
}
