//! Sanitization of untrusted text before it reaches a terminal or a model.
//!
//! Message bodies, titles, names and other fields on the fabric are written
//! by other agents. Printed raw, terminal escape sequences in them can
//! rewrite what a human sees (e.g. a decision title right before approval);
//! returned raw to a model, invisible control characters can hide content.
//! [`sanitize_text`] removes ANSI/OSC escape sequences, C0/C1 control
//! characters (except `\n` and `\t`) and Unicode bidirectional overrides.

const ESC: char = '\u{1b}';
const BEL: char = '\u{07}';
const C1_ST: char = '\u{9c}';

/// Unicode bidirectional embedding/override/isolate controls, which can
/// visually reorder text in a terminal.
fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

/// Skip a control *string* (OSC, DCS, SOS, PM, APC) up to and including its
/// terminator: BEL, ST (`ESC \`) or the C1 ST character.
fn skip_control_string(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while let Some(c) = chars.next() {
        match c {
            BEL | C1_ST => return,
            ESC => {
                if chars.peek() == Some(&'\\') {
                    chars.next();
                }
                return;
            }
            _ => {}
        }
    }
}

/// Skip the remainder of a CSI sequence: parameter and intermediate bytes,
/// then one final byte in `0x40..=0x7e`.
fn skip_csi(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    for c in chars.by_ref() {
        if ('\u{40}'..='\u{7e}').contains(&c) {
            return;
        }
        if !('\u{20}'..='\u{3f}').contains(&c) {
            // Malformed sequence: stop consuming at the first byte that is
            // neither a parameter/intermediate nor a final byte.
            return;
        }
    }
}

/// Strip terminal escape sequences and control characters from untrusted
/// text. Keeps `\n` and `\t`; removes everything else in C0/C1, DEL, and
/// bidirectional override characters.
pub fn sanitize_text(input: &str) -> String {
    // Fast path: nothing to strip.
    if !input
        .chars()
        .any(|c| (c.is_control() && c != '\n' && c != '\t') || is_bidi_control(c))
    {
        return input.to_string();
    }

    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ESC => match chars.next() {
                Some('[') => skip_csi(&mut chars),
                Some(']') | Some('P') | Some('X') | Some('^') | Some('_') => {
                    skip_control_string(&mut chars)
                }
                Some(n) if ('\u{20}'..='\u{2f}').contains(&n) => {
                    // nF escape: intermediates then one final byte.
                    for f in chars.by_ref() {
                        if !('\u{20}'..='\u{2f}').contains(&f) {
                            break;
                        }
                    }
                }
                // Two-character escape (ESC + final byte) or a dangling ESC.
                _ => {}
            },
            '\u{9b}' => skip_csi(&mut chars),
            '\u{90}' | '\u{98}' | '\u{9d}' | '\u{9e}' | '\u{9f}' => skip_control_string(&mut chars),
            '\n' | '\t' => out.push(c),
            c if c.is_control() || is_bidi_control(c) => {}
            c => out.push(c),
        }
    }
    out
}

/// Sanitize every string (and object key) inside a JSON value in place.
pub fn sanitize_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(s) => {
            let clean = sanitize_text(s);
            if clean != *s {
                *s = clean;
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(sanitize_json),
        serde_json::Value::Object(map) => {
            let needs_key_fix = map.keys().any(|k| sanitize_text(k) != *k);
            if needs_key_fix {
                let old = std::mem::take(map);
                for (k, mut v) in old {
                    sanitize_json(&mut v);
                    map.insert(sanitize_text(&k), v);
                }
            } else {
                map.values_mut().for_each(sanitize_json);
            }
        }
        _ => {}
    }
}

/// Sanitize for a single-line display context (e.g. a table cell): like
/// [`sanitize_text`], with newlines and tabs collapsed to spaces.
pub fn sanitize_line(input: &str) -> String {
    sanitize_text(input).replace(['\n', '\t'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_plain_text_newlines_and_tabs() {
        assert_eq!(sanitize_text("hello\n\tworld"), "hello\n\tworld");
        assert_eq!(sanitize_text("héllo wörld ✓"), "héllo wörld ✓");
    }

    #[test]
    fn strips_csi_sequences() {
        assert_eq!(sanitize_text("\x1b[31mred\x1b[0m"), "red");
        assert_eq!(sanitize_text("a\x1b[2K\x1b[1Ab"), "ab");
        assert_eq!(sanitize_text("x\u{9b}31my"), "xy");
    }

    #[test]
    fn strips_osc_sequences() {
        // OSC 8 hyperlink terminated by ST, and title set terminated by BEL.
        assert_eq!(
            sanitize_text("\x1b]8;;http://x\x1b\\link\x1b]8;;\x1b\\"),
            "link"
        );
        assert_eq!(sanitize_text("\x1b]0;title\x07after"), "after");
        // Unterminated OSC swallows the rest.
        assert_eq!(sanitize_text("ok\x1b]0;never ends"), "ok");
    }

    #[test]
    fn strips_control_characters() {
        assert_eq!(sanitize_text("a\rb\x08c\x00d\x7fe"), "abcde");
        assert_eq!(sanitize_text("a\u{85}b"), "ab");
        assert_eq!(sanitize_text("dangling\x1b"), "dangling");
        assert_eq!(sanitize_text("\x1b(Bx"), "x");
        assert_eq!(sanitize_text("\x1bcreset"), "reset");
    }

    #[test]
    fn strips_bidi_overrides() {
        assert_eq!(sanitize_text("abc\u{202e}fed"), "abcfed");
        assert_eq!(sanitize_text("\u{2066}x\u{2069}"), "x");
    }

    #[test]
    fn sanitize_line_collapses_whitespace() {
        assert_eq!(sanitize_line("a\nb\tc\x1b[1m"), "a b c");
    }

    #[test]
    fn sanitizes_json_strings_and_keys() {
        let mut v = serde_json::json!({
            "body": "hi\x1b[2J",
            "list": ["\x1b]0;t\x07ok", 3],
            "k\x1b[1m": "v"
        });
        sanitize_json(&mut v);
        assert_eq!(v["body"], "hi");
        assert_eq!(v["list"][0], "ok");
        assert_eq!(v["list"][1], 3);
        assert_eq!(v["k"], "v");
    }
}
