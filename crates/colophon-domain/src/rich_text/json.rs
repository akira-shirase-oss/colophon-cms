//! Writing canonical JSON: the same bytes JavaScript's `JSON.stringify` produces for the same
//! value, so the editor and the server agree byte for byte.

/// Appends `text` as a JSON string. Escapes exactly what `JSON.stringify` escapes: `"`, `\`,
/// the five short escapes, and the other control characters below U+0020 as `\u00xx`
/// (lowercase hex). Everything else, including non-ASCII, U+2028 and U+2029, is written as is.
pub(crate) fn write_string(out: &mut String, text: &str) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if u32::from(control) < 0x20 => {
                let code = u32::from(control) as usize;
                out.push_str("\\u00");
                out.push(char::from(HEX[code >> 4]));
                out.push(char::from(HEX[code & 0xf]));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

/// How many bytes [`write_string`] writes for `text`, quotes included, without writing them: a
/// size check can refuse text before it is escaped.
pub(crate) fn escaped_len(text: &str) -> usize {
    text.chars().fold(2, |length, character| {
        length.saturating_add(match character {
            '"' | '\\' | '\u{8}' | '\u{c}' | '\n' | '\r' | '\t' => 2,
            control if u32::from(control) < 0x20 => 6,
            other => other.len_utf8(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::{escaped_len, write_string};

    #[test]
    fn escaped_len_is_what_write_string_writes() {
        let every_kind: String = (0_u32..0x80)
            .filter_map(char::from_u32)
            .chain(['é', '€', '\u{2028}', '😀'])
            .collect();
        for text in ["", "plain", "\"q\\", every_kind.as_str()] {
            assert_eq!(escaped_len(text), written(text).len(), "{text:?}");
        }
    }

    fn written(text: &str) -> String {
        let mut out = String::new();
        write_string(&mut out, text);
        out
    }

    #[test]
    fn escapes_like_json_stringify() {
        // Expected values are what `JSON.stringify` returns in Node 24 for the same strings.
        let cases = [
            ("plain", r#""plain""#),
            ("quote\" backslash\\", r#""quote\" backslash\\""#),
            ("\u{8}\u{c}\n\r\t", r#""\b\f\n\r\t""#),
            ("\u{0}\u{1}\u{1f}", r#""\u0000\u0001\u001f""#),
            ("\u{7f}", "\"\u{7f}\""),
            ("é 🚀 \u{2028}\u{2029}", "\"é 🚀 \u{2028}\u{2029}\""),
            ("slash /", r#""slash /""#),
        ];
        for (input, expected) in cases {
            assert_eq!(written(input), expected, "{input:?}");
        }
    }
}
