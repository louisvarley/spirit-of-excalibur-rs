//! LZSS decoder matching the original `1057:0000` routine.
//!
//! Token stream:
//! - one flag byte, consumed least-significant bit first;
//! - flag 1: copy one literal byte from input;
//! - flag 0: two bytes `b`, `c`:
//!   `length = (b & 0x3f) + 3`, `offset = ((b >> 6) & 3) | (c << 2)`;
//!   `offset == 0` terminates the stream; otherwise copy `length` bytes from
//!   `output.len() - offset`.
//!
//! This is the 12-bit-offset / 6-bit-length variant found in the disassembly.

use crate::ResError;

/// A pluggable resource decompressor.
pub trait Decompressor {
    /// Decompress `input` to exactly `expanded_size` bytes.
    fn decompress(&self, input: &[u8], expanded_size: usize) -> Result<Vec<u8>, ResError>;
}

/// Pixel/data decompressor for the `.RES` payloads.
#[derive(Debug, Default, Clone, Copy)]
pub struct LzssDecompressor;

impl Decompressor for LzssDecompressor {
    fn decompress(&self, input: &[u8], expanded_size: usize) -> Result<Vec<u8>, ResError> {
        let mut out = Vec::with_capacity(expanded_size);
        let mut i = 0usize;
        while out.len() < expanded_size {
            if i >= input.len() {
                return Err(ResError::Decompress(format!(
                    "input ended after {} of {} output bytes",
                    out.len(),
                    expanded_size
                )));
            }
            let flags = input[i];
            i += 1;
            for bit in 0..8 {
                if out.len() >= expanded_size {
                    break;
                }
                if flags & (1 << bit) != 0 {
                    if i >= input.len() {
                        return Err(ResError::Decompress("truncated literal".into()));
                    }
                    out.push(input[i]);
                    i += 1;
                } else {
                    if i + 1 >= input.len() {
                        return Err(ResError::Decompress("truncated back-reference".into()));
                    }
                    let b = input[i];
                    let c = input[i + 1];
                    i += 2;
                    let length = usize::from(b & 0x3f) + 3;
                    let offset = usize::from((b >> 6) & 0x3) | (usize::from(c) << 2);
                    if offset == 0 {
                        return Ok(out);
                    }
                    if offset > out.len() {
                        return Err(ResError::Decompress(format!(
                            "back-reference offset {offset} exceeds output {}",
                            out.len()
                        )));
                    }
                    let start = out.len() - offset;
                    for k in 0..length {
                        let byte = out[start + k];
                        out.push(byte);
                    }
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_and_backref() {
        // flags 0b0000_0111: three literals, then a back-reference.
        let payload = [0b0000_0111, b'A', b'B', b'C', 0b1100_0011, 0];
        let out = LzssDecompressor
            .decompress(&payload, 9)
            .expect("decompress");
        assert_eq!(out, b"ABCABCABC");
    }

    #[test]
    fn offset_zero_ends_stream() {
        let payload = [0b0000_0000, 0, 0];
        let out = LzssDecompressor
            .decompress(&payload, 16)
            .expect("decompress");
        assert!(out.is_empty());
    }
}

/// Secondary flag-0x01 RLE codec used by ANI/LOGO/EXTER/EXTER2 resources.
///
/// Tokens: `0x80` = no-op; `0x00..=0x7f` = copy the next `token+1` literal bytes;
/// `0x81..=0xff` = repeat the next byte `0x101-token` times. Decoding stops at
/// `max_output` or end of input.
pub fn rle_decompress(input: &[u8], max_output: usize) -> Result<Vec<u8>, ResError> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < input.len() && out.len() < max_output {
        let token = input[i];
        i += 1;
        match token {
            0x80 => {}
            0x00..=0x7f => {
                let n = usize::from(token) + 1;
                for _ in 0..n {
                    if out.len() >= max_output {
                        break;
                    }
                    out.push(input.get(i).copied().unwrap_or(0));
                    i += 1;
                }
            }
            0x81..=0xff => {
                let n = 0x101 - usize::from(token);
                let value = input.get(i).copied().unwrap_or(0);
                i += 1;
                let take = n.min(max_output - out.len());
                out.extend(std::iter::repeat_n(value, take));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod rle_tests {
    use super::*;

    #[test]
    fn literal_and_repeat_runs() {
        let input = [2, b'A', b'B', b'C', 0xfe, b'Z'];
        let out = rle_decompress(&input, 64).unwrap();
        assert_eq!(out, b"ABCZZZ");
    }

    #[test]
    fn skip_byte_is_noop() {
        let input = [0x80, 0, b'Q'];
        assert_eq!(rle_decompress(&input, 8).unwrap(), b"Q");
    }
}
