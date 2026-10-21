//! Character name phrases used in messages.
//!
//! `display_name` is `FUN_2d65_08a4` ("the townsman", "Sir Kay",
//! "several soldiers"); `article` is `FUN_15b8_16fe` ("", "a ", "an ",
//! "several "). Both read the character records in DGROUP at
//! `DS:2C5E + 0x38 * id` (the name). `encounter_sentence` assembles the
//! `DS:707A` / `DS:70A7` format the encounter dialog prints.

#![forbid(unsafe_code)]

use crate::encounter::EncounterDialog;

const CHAR_NAME: usize = 0x2c5e;
const CHAR_SIZE: usize = 0x38;
const FLAGS: usize = 0x16;
/// Byte count used by `FUN_2d65_08a4`.
const DISPLAY_COUNT: usize = 0x1c;
/// Word count used by the article and plural logic.
const COUNT: usize = 0x1e;

fn name_at(id: u8) -> usize {
    CHAR_NAME + usize::from(id) * CHAR_SIZE
}

fn c_str(ds: &[u8], at: usize) -> String {
    let end = ds[at..].iter().position(|&b| b == 0).map_or(ds.len(), |n| at + n);
    ds[at..end].iter().map(|&b| char::from(b)).collect()
}

fn word(ds: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([ds[at], ds[at + 1]])
}

fn far_string(ds: &[u8], table: usize, index: usize) -> String {
    c_str(ds, usize::from(word(ds, table + index * 4)))
}

/// The character's bare name.
#[must_use]
pub fn name(ds: &[u8], id: u8) -> String {
    c_str(ds, name_at(id))
}

/// `FUN_15b8_16fe(name, count)`: the article before a name.
#[must_use]
pub fn article(name: &str, count: u16) -> &'static str {
    if count > 1 {
        return "several ";
    }
    match name.bytes().next() {
        Some(b'a' | b'e' | b'i' | b'o' | b'u') => "an ",
        Some(c) if c > b'a' => "a ",
        _ => "",
    }
}

/// `FUN_2d65_08a4(id)`: title or "several", the name, and a plural "s".
#[must_use]
pub fn display_name(ds: &[u8], id: u8) -> String {
    let n = name_at(id);
    let count = ds[n + DISPLAY_COUNT];
    let mut out = if count > 1 {
        "several ".to_string()
    } else {
        // DS:79C0 far pointers: "", "the ", "Sir ", "King ".
        let title = usize::from((word(ds, n + FLAGS) as i16 >> 6) as u16 & 3);
        far_string(ds, 0x79c0, title)
    };
    out.push_str(&name(ds, id));
    if count > 1 {
        out.push_str(&c_str(ds, 0x7b22));
    }
    out
}

/// The encounter dialog text (`FUN_2920_0a12` -> `FUN_1772_000e`):
/// `"%s%s%c encounter%c %s%s%c."` plus `"\nWhat shall %s do?"` when the
/// dialog offers a choice. A `%c` of 0 prints nothing.
#[must_use]
pub fn encounter_sentence(ds: &[u8], d: &EncounterDialog) -> String {
    let part = |id: u8| {
        let n = name(ds, id);
        let count = word(ds, name_at(id) + COUNT);
        let plural = count as i16 > 1;
        (format!("{}{n}", article(&n, count)), plural)
    };
    let (first, first_plural) = part(d.first);
    let (second, second_plural) = part(d.second);
    let s = |on: bool| if on { "s" } else { "" };
    let mut text = format!(
        "{first}{} encounter{} {second}{}.",
        s(first_plural),
        s(!first_plural),
        s(second_plural)
    );
    if d.choice {
        text.push_str("\nWhat shall ");
        text.push_str(&far_string(ds, 0x79e0, d.prefix));
        text.push_str(" do?");
    }
    text
}

/// "Stop to talk to " (`DS:7051`) followed by the display name.
#[must_use]
pub fn talk_label(ds: &[u8], second: u8) -> String {
    format!("Stop to talk to {}", display_name(ds, second))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// Compares with strings returned by the original for every character
    /// (`tests/oracle/names_vectors.py`).
    #[test]
    fn matches_original_names() {
        let Ok(text) = std::fs::read_to_string("../../tests/vectors/names.json") else {
            eprintln!("SKIP: tests/vectors/names.json absent");
            return;
        };
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let ds = hex(v["dgroup"].as_str().unwrap());
        let mut n = 0;
        for (id, want) in v["names"].as_object().unwrap() {
            let id: u8 = id.parse().unwrap();
            assert_eq!(display_name(&ds, id), want["display"].as_str().unwrap(), "display {id}");
            let count = word(&ds, name_at(id) + COUNT);
            assert_eq!(article(&name(&ds, id), count), want["article"].as_str().unwrap(), "article {id}");
            n += 1;
        }
        eprintln!("{n} characters' names matched");
    }

    #[test]
    fn articles() {
        assert_eq!(article("townsman", 1), "a ");
        assert_eq!(article("abbot", 0), "an ");
        assert_eq!(article("Kay", 1), "");
        assert_eq!(article("soldier", 3), "several ");
    }
}
