//! Game text formatter, ported from `FUN_14b0_0451`.
//!
//! This is not libc `printf`. Supported conversions:
//! `d`, `u`, `l`, `c`, `s`, `S`, `R`, `W`, `V`, `N`, plus a numeric width
//! prefix (`%5d`) that positions output rather than padding the string.
//!
//! The disassembly swallows `%%` and unknown conversions (it emits nothing).
//! `%S` upper-cases the first character when it is greater than `?`.
//! `%W` opens a right-margin region; `%V` closes it and emits `^` only when a
//! margin was open. `%N` consumes the arguments of the following conversion
//! without emitting it.
//!
//! Width effects are positional in the original renderer, so this string
//! formatter parses and ignores the digits. Wrapping and glyph metrics belong
//! to the text renderer.

#![forbid(unsafe_code)]

/// One variadic argument, typed according to the conversion that consumes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arg {
    /// `%d`, `%u`, `%l`, `%R` (resource id) and the width/list values.
    Number(u32),
    /// `%c`.
    Char(u8),
    /// `%s`, `%S`.
    Text(String),
}

/// Format `format` with `args`. `resolve` maps `%R` resource ids to text.
#[must_use]
pub fn format_message(
    format: &str,
    args: &[Arg],
    mut resolve: impl FnMut(u16) -> Option<String>,
) -> String {
    let bytes = format.as_bytes();
    let mut out = String::with_capacity(bytes.len());
    let mut arg = 0usize;
    let mut i = 0usize;
    let mut margin = false;

    while i < bytes.len() {
        if bytes[i] != b'%' {
            out.push(char::from(bytes[i]));
            i += 1;
            continue;
        }

        i += 1;
        if i >= bytes.len() {
            break;
        }

        // Numeric width prefix; positional in the original, not string padding.
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }

        let spec = bytes[i];
        i += 1;

        if spec == b'N' {
            // Skip the argument(s) of the following conversion.
            if i < bytes.len() {
                let skipped = bytes[i];
                i += 1;
                match skipped {
                    b'S' | b's' => arg += 1,
                    b'R' | b'W' | b'c' | b'd' | b'l' => arg += 1,
                    _ => {}
                }
            }
            continue;
        }

        match spec {
            b'd' => {
                if let Some(Arg::Number(v)) = args.get(arg) {
                    out.push_str(&(*v as u16 as i16).to_string());
                    arg += 1;
                }
            }
            b'u' => {
                if let Some(Arg::Number(v)) = args.get(arg) {
                    // The original routes `%u` through the same signed
                    // base-10 conversion as `%d`.
                    out.push_str(&(*v as u16 as i16).to_string());
                    arg += 1;
                }
            }
            b'l' => {
                if let Some(Arg::Number(v)) = args.get(arg) {
                    out.push_str(&(*v as i32).to_string());
                    arg += 1;
                }
            }
            b'c' => {
                if let Some(Arg::Char(c)) = args.get(arg) {
                    out.push(char::from(*c));
                    arg += 1;
                }
            }
            b's' | b'S' => {
                if let Some(Arg::Text(text)) = args.get(arg) {
                    if spec == b'S' {
                        let mut chars = text.chars();
                        if let Some(first) = chars.next() {
                            if first > '?' {
                                out.push(first.to_ascii_uppercase());
                            } else {
                                out.push(first);
                            }
                            out.extend(chars);
                        }
                    } else {
                        out.push_str(text);
                    }
                    arg += 1;
                }
            }
            b'R' => {
                if let Some(Arg::Number(v)) = args.get(arg) {
                    if let Some(text) = resolve(*v as u16) {
                        out.push_str(&text);
                    }
                    arg += 1;
                }
            }
            b'W' => {
                if let Some(Arg::Number(v)) = args.get(arg) {
                    // The original stores `value - 17`; zero means no margin.
                    margin = *v > 17;
                    arg += 1;
                }
            }
            b'V' => {
                if margin {
                    out.push('^');
                }
                margin = false;
            }
            // `%%` and unknown conversions emit nothing.
            _ => {}
        }
    }

    if margin {
        out.push('^');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(format: &str, args: &[Arg]) -> String {
        format_message(format, args, |_| None)
    }

    #[test]
    fn plain_and_percent_s() {
        assert_eq!(fmt("Hello %s", &[Arg::Text("world".into())]), "Hello world");
        assert_eq!(fmt("100%% done", &[]), "100 done");
    }

    #[test]
    fn capitalizes_first_name_and_keeps_literal_s() {
        assert_eq!(
            fmt(
                "%S sees %d %Ss",
                &[
                    Arg::Text("arthur".into()),
                    Arg::Number(3),
                    Arg::Text("rat".into())
                ]
            ),
            "Arthur sees 3 Rats"
        );
    }

    #[test]
    fn signed_words_and_long_values() {
        assert_eq!(fmt("%d", &[Arg::Number(0xfffb)]), "-5");
        // Oracle: `%u` also uses the signed base-10 converter.
        assert_eq!(fmt("%u", &[Arg::Number(0xffff)]), "-1");
        assert_eq!(fmt("%l", &[Arg::Number(100_000)]), "100000");
        assert_eq!(fmt("%c", &[Arg::Char(b'Q')]), "Q");
    }

    /// Expected strings captured from `FUN_14b0_0451` under the Unicorn
    /// oracle (`tests/oracle/verify_formatter.py`), with output routed to a
    /// far-return stub.
    #[test]
    fn oracle_formatter_vectors() {
        let cases: [(&str, Vec<Arg>, &str); 9] = [
            (
                "Hi %d %s",
                vec![Arg::Number(42), Arg::Text("world".into())],
                "Hi 42 world",
            ),
            (
                "%S sees %d %Ss",
                vec![
                    Arg::Text("arthur".into()),
                    Arg::Number(3),
                    Arg::Text("rat".into()),
                ],
                "Arthur sees 3 Rats",
            ),
            (
                "%d %u %l %c",
                vec![
                    Arg::Number(0xfffb),
                    Arg::Number(0xffff),
                    Arg::Number(100_000),
                    Arg::Char(b'Q'),
                ],
                "-5 -1 100000 Q",
            ),
            ("%5d gold", vec![Arg::Number(12)], "12 gold"),
            (
                "%NsSighting %S",
                vec![Arg::Text("unused".into()), Arg::Text("arthur".into())],
                "Sighting Arthur",
            ),
            ("%Wquote%V", vec![Arg::Number(280)], "quote^"),
            ("100%% done", vec![], "100 done"),
            ("a%Vb", vec![], "ab"),
            (
                "%s|%S",
                vec![Arg::Text("peace".into()), Arg::Text("arthur".into())],
                "peace|Arthur",
            ),
        ];
        for (format, args, expected) in cases {
            assert_eq!(fmt(format, &args), expected, "format {format:?}");
        }
    }

    #[test]
    fn width_digits_are_ignored() {
        assert_eq!(fmt("%5d gold", &[Arg::Number(12)]), "12 gold");
    }

    #[test]
    fn n_skips_following_argument() {
        assert_eq!(
            fmt(
                "%NsSighting %S",
                &[Arg::Text("unused".into()), Arg::Text("arthur".into())]
            ),
            "Sighting Arthur"
        );
    }

    #[test]
    fn w_v_emit_caret_only_when_open() {
        assert_eq!(fmt("a%Vb", &[]), "ab");
        assert_eq!(fmt("%Wquote%V", &[Arg::Number(280)]), "quote^");
    }

    #[test]
    fn resource_lookup() {
        let text = format_message(
            "%S speaks:\n%W\"%R\" ",
            &[
                Arg::Text("arthur".into()),
                Arg::Number(280),
                Arg::Number(42),
            ],
            |id| (id == 42).then(|| "Peace".to_string()),
        );
        assert_eq!(text, "Arthur speaks:\n\"Peace\" ^");
    }
}
