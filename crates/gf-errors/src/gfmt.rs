//! A small `fmt.Sprintf` subset used for deferred error text formatting.
//!
//! Error text and arguments are kept apart (as in Go) so translation layers can
//! replace the text and format it later. Supported verbs: `%v`, `%s`, `%d`,
//! `%f`, `%t`, `%q`, `%x`, `%c`, `%%`, with the `-`, `+`, `0` flags and
//! width/precision. Unsupported verbs render Go's `%!verb(type=value)` marker.

use gf_core::Value;

struct Spec {
    minus: bool,
    plus: bool,
    zero: bool,
    width: Option<usize>,
    precision: Option<usize>,
}

/// Formats `format` with `args`, following Go's `fmt.Sprintf` for the supported
/// verbs.
pub fn sprintf(format: &str, args: &[Value]) -> String {
    let mut out = String::with_capacity(format.len());
    let mut chars = format.chars().peekable();
    let mut arg_index = 0;
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut spec = Spec {
            minus: false,
            plus: false,
            zero: false,
            width: None,
            precision: None,
        };
        while let Some(flag) = chars.peek().copied() {
            match flag {
                '-' => spec.minus = true,
                '+' => spec.plus = true,
                '0' => spec.zero = true,
                '#' => {}
                _ => break,
            }
            let _ = chars.next();
        }
        let mut width = String::new();
        while let Some(d) = chars.peek().copied() {
            if d.is_ascii_digit() {
                width.push(d);
                let _ = chars.next();
            } else {
                break;
            }
        }
        if !width.is_empty() {
            spec.width = width.parse::<usize>().ok();
        }
        if chars.peek() == Some(&'.') {
            let _ = chars.next();
            let mut precision = String::new();
            while let Some(d) = chars.peek().copied() {
                if d.is_ascii_digit() {
                    precision.push(d);
                    let _ = chars.next();
                } else {
                    break;
                }
            }
            spec.precision = precision.parse::<usize>().ok();
        }
        let Some(verb) = chars.next() else {
            out.push('%');
            break;
        };
        if verb == '%' {
            out.push('%');
            continue;
        }
        let Some(value) = args.get(arg_index) else {
            out.push_str(&format!("%!{verb}(MISSING)"));
            continue;
        };
        arg_index += 1;
        let rendered = render(verb, value, &spec);
        push_padded(&mut out, &rendered, &spec);
    }
    out
}

fn render(verb: char, value: &Value, spec: &Spec) -> String {
    match verb {
        'v' | 's' => match (verb, value) {
            ('s', Value::Bytes(bytes)) => String::from_utf8_lossy(bytes).into_owned(),
            ('s', Value::String(s)) => s.clone(),
            _ => value.to_string(),
        },
        'd' => match value {
            Value::Int(v) => v.to_string(),
            Value::UInt(v) => v.to_string(),
            _ => unsupported(verb, value),
        },
        'f' => {
            let n = match value {
                Value::Float(v) => *v,
                Value::Int(v) => *v as f64,
                Value::UInt(v) => *v as f64,
                _ => return unsupported(verb, value),
            };
            let precision = spec.precision.unwrap_or(6);
            if spec.plus {
                format!("{n:+.precision$}")
            } else {
                format!("{n:.precision$}")
            }
        }
        't' => match value {
            Value::Bool(v) => v.to_string(),
            _ => unsupported(verb, value),
        },
        'q' => quote(&value.to_string()),
        'x' => match value {
            Value::Int(v) => format!("{v:x}"),
            Value::UInt(v) => format!("{v:x}"),
            Value::Bytes(bytes) => {
                let mut out = String::with_capacity(bytes.len() * 2);
                for byte in bytes {
                    out.push_str(&format!("{byte:02x}"));
                }
                out
            }
            Value::String(s) => {
                let mut out = String::with_capacity(s.len() * 2);
                for byte in s.as_bytes() {
                    out.push_str(&format!("{byte:02x}"));
                }
                out
            }
            _ => unsupported(verb, value),
        },
        'c' => match value {
            Value::Int(v) if (0..=0x10FFFF).contains(v) => char::from_u32(*v as u32)
                .map(String::from)
                .unwrap_or_default(),
            Value::UInt(v) if *v <= 0x10FFFF => char::from_u32(*v as u32)
                .map(String::from)
                .unwrap_or_default(),
            _ => unsupported(verb, value),
        },
        other => unsupported(other, value),
    }
}

fn unsupported(verb: char, value: &Value) -> String {
    format!("%!{verb}({}={value})", value.go_type_name())
}

fn push_padded(out: &mut String, rendered: &str, spec: &Spec) {
    let Some(width) = spec.width else {
        out.push_str(rendered);
        return;
    };
    let len = rendered.chars().count();
    if len >= width {
        out.push_str(rendered);
        return;
    }
    let pad = width - len;
    let fill = if spec.zero && !spec.minus { '0' } else { ' ' };
    if spec.minus {
        out.push_str(rendered);
        out.extend(std::iter::repeat_n(fill, pad));
    } else {
        out.extend(std::iter::repeat_n(fill, pad));
        out.push_str(rendered);
    }
}

/// Quotes `s` the way Go's `%q` does for printable text.
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0007}' => out.push_str("\\a"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000C}' => out.push_str("\\f"),
            '\u{000B}' => out.push_str("\\v"),
            c if (c as u32) < 0x20 || (c as u32) == 0x7F => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int(v: i64) -> Value {
        Value::Int(v)
    }

    #[test]
    fn verbs_match_go_formatting() {
        assert_eq!(sprintf("%d", &[int(1)]), "1");
        assert_eq!(sprintf("%s", &[Value::String("x".into())]), "x");
        assert_eq!(sprintf("%v", &[int(7)]), "7");
        assert_eq!(sprintf("%t", &[Value::Bool(true)]), "true");
        assert_eq!(sprintf("%q", &[Value::String("a\"b".into())]), "\"a\\\"b\"");
    }

    #[test]
    fn flags_width_and_precision() {
        assert_eq!(sprintf("%5d", &[int(1)]), "    1");
        assert_eq!(sprintf("%-5d|", &[int(1)]), "1    |");
        assert_eq!(sprintf("%05d", &[int(1)]), "00001");
        assert_eq!(sprintf("%.2f", &[Value::Float(1.2345)]), "1.23");
        assert_eq!(sprintf("%+.1f", &[Value::Float(1.25)]), "+1.2");
        assert_eq!(sprintf("%8.3f", &[Value::Float(2.5)]), "   2.500");
    }

    #[test]
    fn literals_and_missing_arguments() {
        assert_eq!(sprintf("100%% ok", &[]), "100% ok");
        assert_eq!(sprintf("%s-%d", &[int(1)]), "1-%!d(MISSING)");
        assert_eq!(sprintf("tail %", &[]), "tail %");
    }

    #[test]
    fn unsupported_verbs_mark_type() {
        assert_eq!(sprintf("%d", &[Value::Bool(true)]), "%!d(bool=true)");
    }

    #[test]
    fn hex_and_char_verbs() {
        assert_eq!(sprintf("%x", &[int(255)]), "ff");
        assert_eq!(sprintf("%x", &[Value::Bytes(vec![0xde, 0xad])]), "dead");
        assert_eq!(sprintf("%c", &[int(65)]), "A");
    }

    #[test]
    fn quote_escapes_control_characters() {
        assert_eq!(quote("a\nb\tc"), "\"a\\nb\\tc\"");
        assert_eq!(quote("\u{0001}"), "\"\\x01\"");
        assert_eq!(quote("测试"), "\"测试\"");
    }
}
