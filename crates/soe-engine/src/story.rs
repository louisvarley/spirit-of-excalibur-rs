//! `HINT.RES` story/hint paragraphs.
//!
//! The archive stores a fragmented text stream: printable runs are separated by
//! NULs and small binary offsets. The game presents these as hint/story
//! paragraphs. We keep long printable runs and discard the binary separators.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::path::Path;

use soe_res::{Decompressor, LzssDecompressor, MAX_REASONABLE_EXPANDED_SIZE, rle_decompress};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HintBook {
    pub paragraphs: Vec<String>,
    /// `HINT.RES` record id -> dialogue line. The event table selects these by
    /// 1-based id, so keeping the record identity matters.
    pub lines: BTreeMap<u16, String>,
}

impl HintBook {
    /// Parse a complete `HINT.RES` file.
    pub fn from_res(bytes: Vec<u8>) -> Result<Self, String> {
        let mut stream = Vec::new();
        let mut lines = BTreeMap::new();
        for (id, data) in hint_records(&bytes)? {
            if let Some(text) = record_text(&data) {
                lines.insert(id, text);
            }
            stream.extend_from_slice(&data);
        }
        Ok(Self {
            paragraphs: extract_runs(&stream),
            lines,
        })
    }

    /// Text of one `HINT.RES` record. Event lines are 1-based record ids.
    #[must_use]
    pub fn line(&self, id: u16) -> Option<&str> {
        self.lines.get(&id).map(String::as_str)
    }

    /// Load `HINT.RES` from disk.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        Self::from_res(bytes)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.paragraphs.is_empty()
    }
}

/// Walk `HINT.RES` directly.
///
/// Directory offsets point sixteen bytes before the record's length word.
/// Uncompressed payloads start at `offset + 0x14`; LZSS records carry an extra
/// expanded-size word and start at `offset + 0x18`. This differs from the
/// generic `.RES` reader, which is why the story book keeps its own walk.
fn hint_records(data: &[u8]) -> Result<Vec<(u16, Vec<u8>)>, String> {
    if data.len() < 0x14 {
        return Err("HINT.RES too short".into());
    }
    let index_offset = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
    if index_offset > data.len() {
        return Err("HINT.RES index out of range".into());
    }
    let dir = &data[index_offset..];
    let mut best: Vec<(u16, u32, u8)> = Vec::new();
    let limit = dir.len().min(0x100).saturating_sub(12);
    for start in 0x10..=limit {
        let mut p = start;
        let mut records = Vec::new();
        while p + 12 <= dir.len() && dir[p + 2..p + 4] == [0xff, 0xff] {
            let id = u16::from_le_bytes([dir[p], dir[p + 1]]);
            let raw = u32::from_le_bytes(dir[p + 4..p + 8].try_into().unwrap());
            records.push((id, raw & 0x00ff_ffff, (raw >> 24) as u8));
            p += 12;
        }
        if records.len() > best.len() {
            best = records;
        }
    }
    if best.is_empty() {
        return Err("HINT.RES has no records".into());
    }
    let mut out = Vec::with_capacity(best.len());
    for (index, (id, offset, flags)) in best.iter().copied().enumerate() {
        let header = if flags & 0x80 != 0 { 0x18 } else { 0x14 };
        let start = offset as usize + header;
        let next = best
            .iter()
            .skip(index + 1)
            .map(|(_, next_offset, next_flags)| {
                *next_offset as usize + if next_flags & 0x80 != 0 { 0x18 } else { 0x14 }
            })
            .find(|next| *next > start)
            .unwrap_or(index_offset);
        if start >= data.len() || next <= start {
            continue;
        }
        let payload = &data[start..next.min(data.len())];
        let decoded = if flags & 0x80 != 0 {
            LzssDecompressor
                .decompress(payload, MAX_REASONABLE_EXPANDED_SIZE as usize)
                .map_err(|e| e.to_string())?
        } else if flags == 0x01 {
            rle_decompress(payload, MAX_REASONABLE_EXPANDED_SIZE as usize)
                .map_err(|e| e.to_string())?
        } else {
            payload.to_vec()
        };
        out.push((id, decoded));
    }
    Ok(out)
}

fn record_text(data: &[u8]) -> Option<String> {
    let end = data
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(data.len());
    let text = String::from_utf8_lossy(&data[..end]).trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn extract_runs(stream: &[u8]) -> Vec<String> {
    let mut runs = Vec::new();
    let mut current = Vec::new();
    for &byte in stream {
        if byte == b'\n' || byte == b'\t' || (0x20..=0x7e).contains(&byte) {
            current.push(byte);
        } else {
            finish_run(&mut current, &mut runs);
        }
    }
    finish_run(&mut current, &mut runs);
    runs
}

fn finish_run(current: &mut Vec<u8>, runs: &mut Vec<String>) {
    if current.len() >= 24 {
        let text = String::from_utf8_lossy(current);
        let good = text
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || " .,!?'\"-;:".contains(*c))
            .count();
        if good * 100 >= text.len() * 85 {
            runs.push(text.trim().to_string());
        }
    }
    current.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_story_paragraphs() {
        let Ok(bytes) = std::fs::read("../../game/HINT.RES") else {
            eprintln!("SKIP: HINT.RES absent");
            return;
        };
        let book = HintBook::from_res(bytes).unwrap();
        assert!(book.paragraphs.len() > 100);
        assert!(
            book.paragraphs
                .iter()
                .any(|text| text.contains("These are strange times we live in."))
        );
        assert!(
            book.paragraphs
                .iter()
                .any(|text| text.contains("Acclaimed by the remaining knights"))
        );
    }
}
