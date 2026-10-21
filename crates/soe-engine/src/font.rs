//! Variable-width 1bpp font used by the video drivers.
//!
//! From MCGA `drv_2` / `FUN_0000_07b4`:
//! - header: `u8 height, u8 nominal_width, u8 line_spacing`
//! - 96 glyph descriptors for codes `0x20..=0x7f`, three bytes each:
//!   `u8 width, u16 offset` (offset relative to the font base)
//! - glyph rows use `even(ceil(width/8))` mask bytes per row.
//!
//! The mask bytes are bitwise blended with the text colour word by
//! `FUN_0000_06fd`; the exact mask-to-pixel expansion is still under review.
//! The font's actual location is also open; this parser is ready for it.

#![forbid(unsafe_code)]

use soe_video::{IndexedFrame, PlanarImage};

/// Font strip used by the game text formatter.
///
/// The atlas is `TITLE.RES#200` (301x9, 5 planes). Metrics come from EXCAL
/// DGROUP: height at `DS:1E8A`, inter-character spacing at `DS:1E8B`, and 95
/// words at `DS:1E8C` for codes `0x20..=0x7e`.
///
/// Metric word: `src_x = word & 0x3FF`, bit 10 = right overhang, bit 11 =
/// left overhang, `width = word >> 12`. Advance is
/// `width + spacing - bit10 - bit11`.
/// Atlas index of the glyph body in `TITLE.RES#200`.
#[cfg(test)]
const ATLAS_STROKE: u8 = 2;
/// Atlas index of the light drop highlight under each stroke.
const ATLAS_HIGHLIGHT: u8 = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BitmapFont {
    pub width: u16,
    pub height: u16,
    pub spacing: u8,
    pixels: Vec<u8>,
    metrics: Vec<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlyphRef {
    pub src_x: u16,
    pub width: u8,
    pub left_overhang: bool,
    pub right_overhang: bool,
}

impl GlyphRef {
    #[must_use]
    pub fn advance(self, spacing: u8) -> i32 {
        i32::from(self.width) + i32::from(spacing)
            - i32::from(self.left_overhang)
            - i32::from(self.right_overhang)
    }
}

impl BitmapFont {
    /// Build from a decoded atlas and 95 metric words.
    pub fn new(image: PlanarImage, spacing: u8, metrics: &[u16]) -> Result<Self, String> {
        if metrics.len() != 95 {
            return Err(format!("expected 95 font metrics, got {}", metrics.len()));
        }
        if image.height == 0 || image.width == 0 {
            return Err("empty font atlas".into());
        }
        Ok(Self {
            width: image.width,
            height: image.height,
            spacing,
            pixels: image.indices,
            metrics: metrics.to_vec(),
        })
    }

    /// Read metrics from a complete MZ image of `EXCAL_UNPACKED.EXE`.
    pub fn metrics_from_excal(exe: &[u8]) -> Option<(u8, u8, Vec<u16>)> {
        if exe.len() < 0x1c || &exe[..2] != b"MZ" {
            return None;
        }
        let payload = usize::from(u16::from_le_bytes([exe[8], exe[9]])).checked_mul(16)?;
        let base = payload.checked_add(crate::palette::DGROUP_OFFSET)?;
        let height = *exe.get(base + 0x1e8a)?;
        let spacing = *exe.get(base + 0x1e8b)?;
        let raw = exe.get(base + 0x1e8c..base + 0x1e8c + 95 * 2)?;
        let metrics = raw
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        Some((height, spacing, metrics))
    }

    #[must_use]
    pub fn glyph(&self, code: u8) -> Option<GlyphRef> {
        if !(0x20..=0x7e).contains(&code) {
            return None;
        }
        let metric = *self.metrics.get(usize::from(code - 0x20))?;
        Some(GlyphRef {
            src_x: metric & 0x3ff,
            width: (metric >> 12) as u8,
            left_overhang: metric >> 11 & 1 != 0,
            right_overhang: metric >> 10 & 1 != 0,
        })
    }

    /// Advance width of a string in logical pixels.
    #[must_use]
    pub fn text_width(&self, text: &str) -> u32 {
        text.bytes()
            .filter_map(|code| self.glyph(code))
            .map(|glyph| glyph.advance(self.spacing).max(0) as u32)
            .sum()
    }

    /// Draw text at a logical position. Palette index 0 is transparent; when
    /// `color` is set, every non-zero glyph pixel uses that index.
    pub fn draw_text(
        &self,
        frame: &mut IndexedFrame,
        x: i32,
        y: i32,
        text: &str,
        color: Option<u8>,
    ) {
        let atlas_width = i32::from(self.width);
        let atlas_height = i32::from(self.height);
        let line_height = atlas_height + 1;
        let mut pen = x;
        let mut baseline = y;
        for code in text.bytes() {
            if code == b'\n' {
                pen = x;
                baseline += line_height;
                continue;
            }
            let Some(glyph) = self.glyph(code) else {
                pen += 1;
                continue;
            };
            let start_x = pen - i32::from(glyph.left_overhang);
            for row in 0..atlas_height {
                for col in 0..i32::from(glyph.width) {
                    let sx = i32::from(glyph.src_x) + col;
                    if sx >= atlas_width {
                        continue;
                    }
                    let pixel = self.pixels[(row * atlas_width + sx) as usize];
                    // The atlas carries its own shading: stroke 2, edges 3/4,
                    // highlight 6. A solid colour recolours the stroke and edges and drops the
                    // highlight so
                    // the shading does not thicken the glyph on a flat ground.
                    let out = match color {
                        None if pixel != 0 => Some(pixel),
                        Some(color) if pixel != 0 && pixel != ATLAS_HIGHLIGHT => Some(color),
                        _ => None,
                    };
                    if let Some(out) = out {
                        frame.set_pixel((start_x + col) as u16, (baseline + row) as u16, out);
                    }
                }
            }
            pen += glyph.advance(self.spacing);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Glyph {
    pub width: u8,
    pub height: u8,
    pub row_bytes: u8,
    pub rows: Vec<u8>,
}

impl Glyph {
    /// One row as palette-index bits, MSB first.
    #[must_use]
    pub fn row_bits(&self, row: usize) -> Option<impl Iterator<Item = bool> + '_> {
        if row >= usize::from(self.height) {
            return None;
        }
        let stride = usize::from(self.row_bytes);
        let bytes = &self.rows[row * stride..(row + 1) * stride];
        Some((0..usize::from(self.width)).map(move |x| {
            let byte = bytes[x >> 3];
            (byte >> (7 - (x & 7))) & 1 != 0
        }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Font {
    pub height: u8,
    pub nominal_width: u8,
    pub line_spacing: u8,
    glyphs: Vec<Glyph>,
}

impl Font {
    /// Parse a driver font blob.
    pub fn parse(data: &[u8]) -> Result<Self, String> {
        if data.len() < 3 + 96 * 3 {
            return Err("font header/table truncated".into());
        }
        let height = data[0];
        let nominal_width = data[1];
        let line_spacing = data[2];
        if height == 0 || height > 32 {
            return Err(format!("invalid font height {height}"));
        }

        let mut glyphs = Vec::with_capacity(96);
        for code in 0x20u8..=0x7f {
            let entry = 3 + usize::from(code - 0x20) * 3;
            let width = data[entry];
            let offset = usize::from(u16::from_le_bytes([data[entry + 1], data[entry + 2]]));
            let mut row_bytes = (usize::from(width) + 7) / 8;
            row_bytes += row_bytes & 1;
            let length = usize::from(height) * row_bytes;
            let rows = if width == 0 {
                Vec::new()
            } else {
                data.get(offset..offset + length)
                    .ok_or_else(|| format!("glyph {code:#x} outside font blob"))?
                    .to_vec()
            };
            glyphs.push(Glyph {
                width,
                height,
                row_bytes: row_bytes as u8,
                rows,
            });
        }
        Ok(Self {
            height,
            nominal_width,
            line_spacing,
            glyphs,
        })
    }

    /// Glyph for an ASCII code.
    #[must_use]
    pub fn glyph(&self, code: u8) -> Option<&Glyph> {
        if !(0x20..=0x7f).contains(&code) {
            return None;
        }
        self.glyphs.get(usize::from(code - 0x20))
    }

    /// Sum of the glyph widths of a string, in pixels.
    #[must_use]
    pub fn text_width(&self, text: &str) -> u32 {
        text.bytes()
            .filter_map(|code| self.glyph(code))
            .map(|glyph| u32::from(glyph.width))
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_font() -> Vec<u8> {
        let height = 8u8;
        let table = 3 + 96 * 3;
        let mut data = vec![0u8; table + 16];
        data[0] = height;
        data[1] = 8;
        data[2] = 8;
        let index = 3 + usize::from(b'A' - 0x20) * 3;
        data[index] = 8;
        data[index + 1..index + 3].copy_from_slice(&(table as u16).to_le_bytes());
        // 8x8 'A' with a two-byte stride (even-row padding).
        let rows = [
            0b0011_1100,
            0b0110_0110,
            0b1100_0011,
            0b1111_1111,
            0b1100_0011,
            0b1100_0011,
            0b1100_0011,
            0,
        ];
        for (i, byte) in rows.iter().enumerate() {
            data[table + i * 2] = *byte;
        }
        data
    }

    #[test]
    fn parses_variable_width_table_and_rows() {
        let font = Font::parse(&synthetic_font()).unwrap();
        assert_eq!((font.height, font.line_spacing), (8, 8));
        let a = font.glyph(b'A').unwrap();
        assert_eq!((a.width, a.row_bytes, a.rows.len()), (8, 2, 16));
        assert_eq!(
            a.row_bits(0).unwrap().collect::<Vec<_>>(),
            [false, false, true, true, true, true, false, false]
        );
        // Synthetic font leaves space at width 0.
        assert_eq!(font.text_width("A A"), 16);
        assert!(font.glyph(b'B').unwrap().rows.is_empty());
    }

    #[test]
    fn rejects_truncated_table() {
        assert!(Font::parse(&[8, 8, 8]).is_err());
    }

    #[test]
    fn draws_from_atlas_and_metrics() {
        let mut indices = vec![0u8; 16 * 10];
        indices[2] = ATLAS_STROKE;
        indices[3] = ATLAS_STROKE;
        indices[4] = ATLAS_STROKE;
        let atlas = PlanarImage {
            width: 16,
            height: 10,
            planes: 5,
            indices,
        };
        let mut metrics = vec![0u16; 95];
        metrics[0] = (3 << 12) | 2; // ' ': three pixels at x=2
        let font = BitmapFont::new(atlas, 1, &metrics).unwrap();
        let mut frame = IndexedFrame::black();
        font.draw_text(&mut frame, 10, 20, " ", Some(5));
        assert_eq!(frame.pixel(10, 20), Some(5));
        assert_eq!(frame.pixel(11, 20), Some(5));
        assert_eq!(frame.pixel(12, 20), Some(5));
        font.draw_text(&mut frame, 10, 20, " ", None);
        assert_eq!(frame.pixel(10, 20), Some(ATLAS_STROKE));
        font.draw_text(&mut frame, 10, 40, " \n ", None);
        assert_eq!(frame.pixel(10, 51), Some(ATLAS_STROKE));
    }

    #[test]
    fn original_atlas_and_metrics_load() {
        let Ok(res_bytes) = std::fs::read("../../game/RES32/TITLE.RES") else {
            eprintln!("SKIP: TITLE.RES absent");
            return;
        };
        let Ok(exe) = std::fs::read("../../ghidra/EXCAL_UNPACKED.EXE") else {
            eprintln!("SKIP: EXCAL_UNPACKED.EXE absent");
            return;
        };
        let res = soe_res::ResFile::parse(res_bytes).unwrap();
        let record = res.records().into_iter().find(|r| r.id == 200).unwrap();
        let atlas = soe_video::decode_planar(&res.decode(record).unwrap()).unwrap();
        let (height, spacing, metrics) = BitmapFont::metrics_from_excal(&exe).unwrap();
        assert_eq!(height, 10);
        assert_eq!(spacing, 1);
        let font = BitmapFont::new(atlas, spacing, &metrics).unwrap();
        let mut frame = IndexedFrame::black();
        font.draw_text(&mut frame, 0, 0, "Constantine", Some(3));
        assert!(frame.pixels.iter().any(|pixel| *pixel == 3));
        assert!(font.glyph(b'A').unwrap().width > 0);
    }
}
