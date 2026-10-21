//! Reader for the original `.RES` containers.
//!
//! The 16-byte header is verified:
//! `data_start` (0x10), `index_offset`, `data_length = index_offset - 16`,
//! `index_length`, with `file_len = index_offset + index_length`.
//!
//! The record directory layout is only partly understood. The reader exposes
//! the header, an inferred directory-entry count (`(index_length - 16) / 12`),
//! and a best-effort sequential record scan. The first record of HINT.RES and
//! MUSIC.RES is parsed and decompressed by the tests. SCENES.RES uses a
//! different record layout and is handled as raw length-prefixed records.

#![forbid(unsafe_code)]

mod decompress;

pub use decompress::{Decompressor, LzssDecompressor, rle_decompress};

use thiserror::Error;

/// Size of the fixed `.RES` header.
pub const HEADER_SIZE: u32 = 16;
/// Size of the per-record header for the common two-u32 form.
pub const RECORD_HEADER_SIZE: usize = 8;
/// Upper bound used when deciding whether a word looks like an expanded size.
pub const MAX_REASONABLE_EXPANDED_SIZE: u32 = 16 * 1024 * 1024;

/// Errors produced by the container reader.
#[derive(Debug, Error)]
pub enum ResError {
    #[error("file is too short for a .RES header ({0} bytes)")]
    TooShort(usize),
    #[error("invalid header field {field}: {value}")]
    InvalidHeader { field: &'static str, value: u32 },
    #[error(
        "header arithmetic does not match file length: header says {expected}, file is {actual}"
    )]
    LengthMismatch { expected: u64, actual: usize },
    #[error("record at {offset} has invalid length {len}")]
    InvalidRecord { offset: usize, len: u32 },
    #[error("decompression failed: {0}")]
    Decompress(String),
}

/// The fixed 16-byte `.RES` header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResHeader {
    pub data_start: u32,
    pub index_offset: u32,
    pub data_length: u32,
    pub index_length: u32,
}

impl ResHeader {
    /// Parse and validate the header for a byte slice.
    pub fn parse(bytes: &[u8]) -> Result<Self, ResError> {
        if bytes.len() < HEADER_SIZE as usize {
            return Err(ResError::TooShort(bytes.len()));
        }
        let data_start = u32::from_le_bytes(bytes[0..4].try_into().expect("4 bytes"));
        let index_offset = u32::from_le_bytes(bytes[4..8].try_into().expect("4 bytes"));
        let data_length = u32::from_le_bytes(bytes[8..12].try_into().expect("4 bytes"));
        let index_length = u32::from_le_bytes(bytes[12..16].try_into().expect("4 bytes"));
        let header = Self {
            data_start,
            index_offset,
            data_length,
            index_length,
        };
        header.validate(bytes.len())?;
        Ok(header)
    }

    /// Validate the known header invariants.
    pub fn validate(&self, file_len: usize) -> Result<(), ResError> {
        if self.data_start != HEADER_SIZE {
            return Err(ResError::InvalidHeader {
                field: "data_start",
                value: self.data_start,
            });
        }
        if self.index_offset < HEADER_SIZE {
            return Err(ResError::InvalidHeader {
                field: "index_offset",
                value: self.index_offset,
            });
        }
        if self.data_length != self.index_offset - HEADER_SIZE {
            return Err(ResError::InvalidHeader {
                field: "data_length",
                value: self.data_length,
            });
        }
        let expected = u64::from(self.index_offset) + u64::from(self.index_length);
        if expected != file_len as u64 {
            return Err(ResError::LengthMismatch {
                expected,
                actual: file_len,
            });
        }
        Ok(())
    }

    /// Number of 12-byte directory entries, inferred from `index_length`.
    ///
    /// The index region starts with a 16-byte header copy; most files then use
    /// 12-byte entries. TITLE.RES has an 8-byte remainder, so this is a
    /// best-effort count rather than a verified record count.
    #[must_use]
    pub const fn directory_entries(&self) -> u32 {
        if self.index_length < HEADER_SIZE {
            0
        } else {
            (self.index_length - HEADER_SIZE) / 12
        }
    }
}

/// A record discovered by the best-effort sequential scanner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordMeta {
    /// Absolute file offset of the resource payload.
    pub offset: usize,
    /// Resource id from the directory.
    pub id: u16,
    /// Alias kept for older callers: same as `id`.
    pub record_index: u16,
    /// Stored payload length in bytes.
    pub stored_len: usize,
    /// Expanded length when the archive declares one (first resource).
    pub expanded_len: Option<usize>,
    /// Whether the payload is LZSS-compressed (flag 0x80).
    pub compressed: bool,
    /// Header size in the payload (0 for this schema).
    pub header_len: usize,
    /// Directory flags (0x00 raw, 0x80 LZSS, 0x01 secondary codec).
    pub flags: u8,
    /// Type-specific extra word.
    pub extra: u32,
}

/// An in-memory `.RES` file.
#[derive(Debug, Clone)]
pub struct ResFile {
    data: Vec<u8>,
    header: ResHeader,
}

impl ResFile {
    /// Parse an in-memory `.RES` file.
    pub fn parse(data: Vec<u8>) -> Result<Self, ResError> {
        let header = ResHeader::parse(&data)?;
        Ok(Self { data, header })
    }

    /// The parsed header.
    #[must_use]
    pub const fn header(&self) -> ResHeader {
        self.header
    }

    /// Raw file bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.data
    }

    /// Inferred directory-entry count.
    #[must_use]
    pub const fn directory_entries(&self) -> u32 {
        self.header.directory_entries()
    }

    /// Type IDs present in the directory header, in normal reading order.
    /// The file stores them byte-reversed (e.g. `SUMS` means `SMUS`).
    #[must_use]
    pub fn type_ids(&self) -> Vec<String> {
        let io = self.header.index_offset as usize;
        let dir = &self.data[io..];
        let mut out = Vec::new();
        let limit = dir.len().min(0x40);
        for p in 0x1e..limit.saturating_sub(3) {
            let raw = &dir[p..p + 4];
            if raw.iter().all(|b| b.is_ascii_uppercase()) {
                let mut rev = raw.to_vec();
                rev.reverse();
                let id = String::from_utf8_lossy(&rev).to_string();
                if !out.contains(&id) {
                    out.push(id);
                }
            }
        }
        out
    }

    /// Directory-driven record table.
    ///
    /// The directory at `index_offset` contains a 16-byte header copy, a 22-byte
    /// type header, then 12-byte records:
    /// `u16 id, u16 0xFFFF, u24 offset, u8 flags, u32 extra`.
    /// `offset` is relative to the payload base (`0x18`, or `0x14` for the
    /// 5-dword SCENES variant). The resource ends at the next larger offset, or
    /// at the directory offset for the last resource. TITLE/INTF contain
    /// multiple type sections, so the longest consistent 12-byte run is used.
    #[must_use]
    pub fn records(&self) -> Vec<RecordMeta> {
        let d4 = read_u32(&self.data, 0x10);
        let d5 = read_u32(&self.data, 0x14);
        let six = d4 != 0
            && d4 < MAX_REASONABLE_EXPANDED_SIZE
            && d5 >= d4.saturating_add(4)
            && d5 < MAX_REASONABLE_EXPANDED_SIZE;
        let payload_base = if six { 0x18usize } else { 0x14usize };
        self.records_with_payload_base(payload_base, six, d5 as usize)
    }

    /// Directory records with an explicit payload base.
    ///
    /// `HINT.RES` is a five-dword container and its directory offsets are
    /// relative to `0x14`; the generic heuristic above cannot distinguish it
    /// from a six-dword file, so text callers use this entry point.
    #[must_use]
    pub fn records_with_payload_base(
        &self,
        payload_base: usize,
        expanded: bool,
        first_expanded_len: usize,
    ) -> Vec<RecordMeta> {
        let io = self.header.index_offset as usize;
        let dir = &self.data[io..];
        let payload_len = io.saturating_sub(payload_base);

        let mut best: Vec<(u16, u32, u8, u32)> = Vec::new();
        let limit = dir.len().min(0x100).saturating_sub(12);
        for start in 0x10..=limit {
            let mut p = start;
            let mut recs = Vec::new();
            while p + 12 <= dir.len() && dir[p + 2..p + 4] == [0xff, 0xff] {
                let id = u16::from_le_bytes([dir[p], dir[p + 1]]);
                let raw = read_u32(dir, p + 4);
                let offset = raw & 0x00ff_ffff;
                let flags = (raw >> 24) as u8;
                let extra = read_u32(dir, p + 8);
                if offset as usize > payload_len {
                    break;
                }
                recs.push((id, offset, flags, extra));
                p += 12;
            }
            if recs.len() > best.len() {
                best = recs;
            }
        }

        let mut offsets: Vec<u32> = best.iter().map(|r| r.1).collect();
        offsets.sort_unstable();
        offsets.dedup();
        let mut out = Vec::with_capacity(best.len());
        for (id, offset, flags, extra) in best {
            let end = offsets
                .iter()
                .copied()
                .find(|&o| o > offset)
                .unwrap_or(payload_len as u32);
            let expanded_len = (expanded && offset == 0).then_some(first_expanded_len);
            out.push(RecordMeta {
                offset: payload_base + offset as usize,
                id,
                record_index: id,
                stored_len: end.saturating_sub(offset) as usize,
                expanded_len,
                compressed: flags & 0x80 != 0,
                header_len: 0,
                flags,
                extra,
            });
        }
        out
    }

    /// Records for `HINT.RES`.
    ///
    /// Despite the historical "five-dword" name, `HINT.RES` uses the same
    /// six-dword layout as the other text archives: directory offsets are
    /// relative to `0x18` and the first record's expanded length lives in the
    /// dword at `0x14`.
    #[must_use]
    pub fn records_five_dword(&self) -> Vec<RecordMeta> {
        self.records()
    }

    /// Stored payload bytes for a record (no decompression).
    #[must_use]
    pub fn payload(&self, record: RecordMeta) -> &[u8] {
        &self.data[record.offset..record.offset + record.stored_len]
    }

    /// Decode a record using its flag: LZSS (0x80), secondary RLE (0x01), or raw.
    pub fn decode(&self, record: RecordMeta) -> Result<Vec<u8>, ResError> {
        let payload = self.payload(record);
        if record.compressed {
            LzssDecompressor.decompress(payload, MAX_REASONABLE_EXPANDED_SIZE as usize)
        } else if record.flags == 0x01 {
            rle_decompress(payload, MAX_REASONABLE_EXPANDED_SIZE as usize)
        } else {
            Ok(payload.to_vec())
        }
    }

    /// Decompress a record using the supplied decompressor.
    ///
    /// Records without a declared expanded size are returned as-is.
    pub fn decompress_record<D: Decompressor>(
        &self,
        record: RecordMeta,
        decompressor: &D,
    ) -> Result<Vec<u8>, ResError> {
        let payload = self.payload(record);
        match record.expanded_len {
            Some(size) if record.compressed => decompressor.decompress(payload, size),
            _ => Ok(payload.to_vec()),
        }
    }
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("bounds checked by caller"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn game_dir() -> PathBuf {
        std::env::var_os("SOE_GAME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("../../game"))
    }

    fn res_files() -> Vec<PathBuf> {
        let root = game_dir();
        let mut files = Vec::new();
        for dir in [root.clone(), root.join("RES32")] {
            let Ok(entries) = fs::read_dir(dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("RES"))
                {
                    files.push(path);
                }
            }
        }
        files
    }

    #[test]
    fn header_arithmetic_holds_for_all_res_files() {
        let files = res_files();
        if files.is_empty() {
            eprintln!("SKIP: no .RES files under {}", game_dir().display());
            return;
        }
        for path in files {
            let bytes = fs::read(&path).expect("read .RES");
            let header =
                ResHeader::parse(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert_eq!(header.data_start, HEADER_SIZE, "{}", path.display());
            assert_eq!(
                header.data_length,
                header.index_offset - HEADER_SIZE,
                "{}",
                path.display()
            );
            assert_eq!(
                bytes.len() as u64,
                u64::from(header.index_offset) + u64::from(header.index_length),
                "{}",
                path.display()
            );
        }
    }

    #[test]
    fn first_music_record_is_form() {
        let path = game_dir().join("MUSIC.RES");
        if !path.exists() {
            eprintln!("SKIP: {} absent", path.display());
            return;
        }
        let file = ResFile::parse(fs::read(&path).expect("read MUSIC.RES")).expect("parse");
        let rec = *file.records().first().expect("first record");
        let data = file
            .decompress_record(rec, &LzssDecompressor)
            .expect("decompress first record");
        assert!(
            data.starts_with(b"FORM"),
            "head={:?}",
            &data[..4.min(data.len())]
        );
        if let Some(expected) = rec.expanded_len {
            assert_eq!(data.len(), expected);
        }
    }

    #[test]
    fn first_hint_record_is_readable_text() {
        let path = game_dir().join("HINT.RES");
        if !path.exists() {
            eprintln!("SKIP: {} absent", path.display());
            return;
        }
        let file = ResFile::parse(fs::read(&path).expect("read HINT.RES")).expect("parse");
        let rec = *file.records().first().expect("first record");
        let data = file
            .decompress_record(rec, &LzssDecompressor)
            .expect("decompress first record");
        let text = String::from_utf8_lossy(&data);
        assert!(text.contains("Acclaimed"), "text={text:?}");
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn oracle_lzss_vectors() {
        let path = std::env::var_os("SOE_VECTORS")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("../../tests/vectors/decompressors.tsv"));
        let Ok(text) = fs::read_to_string(&path) else {
            eprintln!("SKIP: oracle vectors absent at {}", path.display());
            return;
        };
        let mut n = 0;
        for line in text.lines() {
            let p: Vec<&str> = line.split('\t').collect();
            if p.len() != 5 || p[0] != "128" {
                continue;
            }
            let input = hex(p[3]);
            let expected = hex(p[4]);
            let out = LzssDecompressor
                .decompress(&input, expected.len())
                .expect("lzss");
            assert_eq!(out, expected, "{}#{}", p[1], p[2]);
            n += 1;
        }
        assert!(n > 0, "no LZSS vectors");
    }

    #[test]
    fn index_record_counts_match_the_directory() {
        let cases = [
            ("MUSIC.RES", 25),
            ("HINT.RES", 195),
            ("SCENES.RES", 138),
            ("RES32/ANI.RES", 41),
        ];
        for (name, expected) in cases {
            let path = game_dir().join(name);
            if !path.exists() {
                eprintln!("SKIP: {} absent", path.display());
                continue;
            }
            let file = ResFile::parse(fs::read(&path).expect("read")).expect("parse");
            assert_eq!(file.records().len(), expected, "{name}");
        }
    }

    #[test]
    fn lzss_round_trip_style_vector() {
        // flags 0b0000_0111: three literals, then a 6-byte back-reference at offset 3.
        let payload = [0b0000_0111, b'A', b'B', b'C', 0b1100_0011, 0];
        let out = LzssDecompressor
            .decompress(&payload, 9)
            .expect("decompress");
        assert_eq!(out, b"ABCABCABC");
    }
}
