//! `HINT.RES` story/hint paragraphs.
//!
//! The archive stores a fragmented text stream: printable runs are separated by
//! NULs and small binary offsets. The game presents these as hint/story
//! paragraphs. We keep long printable runs and discard the binary separators.

#![forbid(unsafe_code)]

use std::path::Path;

use soe_res::ResFile;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HintBook {
    pub paragraphs: Vec<String>,
}

impl HintBook {
    /// Parse a complete `HINT.RES` file.
    pub fn from_res(bytes: Vec<u8>) -> Result<Self, String> {
        let file = ResFile::parse(bytes).map_err(|e| e.to_string())?;
        let mut stream = Vec::new();
        for record in file.records() {
            let data = file.decode(record).map_err(|e| e.to_string())?;
            stream.extend_from_slice(&data);
        }
        Ok(Self {
            paragraphs: extract_runs(&stream),
        })
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
