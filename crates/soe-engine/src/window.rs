//! Stone dialog windows and buttons.
//!
//! Ported from `FUN_2036_0009` (window background builder installed in the
//! `DS:0312` callback by `FUN_2d65_2cd9`), `FUN_2036_0475` (bordered, speckled
//! stone area) and the button outlines in `FUN_1772_01ec`.
//!
//! The window art is the 224x20 sheet `TITLE.RES#300` (loaded into `DS:0316`).
//! Piece source rectangles are four 18-byte DGROUP tables: x `DS:5D62`,
//! y `DS:5D74`, width `DS:5D86`, height `DS:5D98`. Colours come from the byte
//! table `DS:1F4D..1F58`.
//!
//! The output is a list of logical drawing operations against the sheet, so
//! the art can be replaced by an override pack without touching this code.
//!
//! Open: blit mode `0x47` is taken to be the horizontally mirrored variant of
//! `0x46` (only used by windows taller than 100 lines); the original RNG
//! (`FUN_15b8_17e0`) is seeded from the timer, so speckles and edge-piece
//! choices are not reproducible and use a local LCG here.

#![forbid(unsafe_code)]

/// Window art sheet: `TITLE.RES#300`.
pub const SHEET_FILE: &str = "TITLE.RES";
pub const SHEET_ID: u16 = 300;

const PIECES_DS: usize = 0x5d62;
const COLORS_DS: usize = 0x1f4d;

/// DGROUP tables used by the window and button routines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowStyle {
    /// Raw bytes `DS:5D62..5DAA` (x, y, width, height tables, 18 each).
    pieces: [u8; 72],
    /// Raw bytes `DS:1F4D..1F59`.
    colors: [u8; 12],
}

/// One drawing operation in window-local logical coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowOp {
    /// Solid rectangle in a palette index (`FUN_14b0_0bbf` mode `0x10`).
    Fill {
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        color: u8,
    },
    /// Copy of a sheet rectangle (mode `0x46`, or `0x47` when `mirror`).
    Piece {
        src_x: u16,
        src_y: u16,
        width: u16,
        height: u16,
        x: i16,
        y: i16,
        mirror: bool,
    },
}

/// A built window: its size, where content starts (`DS:9A01`, `DS:9A03`) and
/// the operations that draw its background.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowLayout {
    pub width: u16,
    pub height: u16,
    pub content_x: i16,
    pub content_y: i16,
    pub ops: Vec<WindowOp>,
}

/// Stand-in for the timer-seeded `FUN_15b8_17e0(n)`: uniform in `0..n`.
struct Rng(u32);

impl Rng {
    fn below(&mut self, n: u16) -> u16 {
        if n == 0 {
            return 0;
        }
        self.0 = self.0.wrapping_mul(0x015a_4e35).wrapping_add(1);
        ((self.0 >> 16) % u32::from(n)) as u16
    }
}

impl WindowStyle {
    /// Read the tables from a complete MZ image of `EXCAL_UNPACKED.EXE`.
    #[must_use]
    pub fn from_exe(exe: &[u8]) -> Option<Self> {
        if exe.len() < 0x1c || &exe[..2] != b"MZ" {
            return None;
        }
        let payload = usize::from(u16::from_le_bytes([exe[8], exe[9]])).checked_mul(16)?;
        let base = payload.checked_add(crate::palette::DGROUP_OFFSET)?;
        Some(Self {
            pieces: exe
                .get(base + PIECES_DS..base + PIECES_DS + 72)?
                .try_into()
                .ok()?,
            colors: exe
                .get(base + COLORS_DS..base + COLORS_DS + 12)?
                .try_into()
                .ok()?,
        })
    }

    /// Colour byte at `DS:addr` for `addr` in `0x1F4D..=0x1F58`.
    #[must_use]
    pub fn color(&self, addr: u16) -> u8 {
        self.colors[usize::from(addr - COLORS_DS as u16)]
    }

    /// Byte at `DS:addr` inside the piece tables.
    fn piece_byte(&self, addr: u16) -> u16 {
        u16::from(self.pieces[usize::from(addr) - PIECES_DS])
    }

    /// Piece op from four separate table addresses, as the original
    /// sometimes mixes indices between the x/y/width/height tables.
    fn piece_at(&self, sx: u16, sy: u16, w: u16, h: u16, x: u16, y: u16, mode: u8) -> WindowOp {
        WindowOp::Piece {
            src_x: self.piece_byte(sx),
            src_y: self.piece_byte(sy),
            width: self.piece_byte(w),
            height: self.piece_byte(h),
            x: x as i16,
            y: y as i16,
            mirror: mode == 0x47,
        }
    }

    /// Piece `index` (0..18) from all four tables.
    fn piece(&self, index: u16, x: u16, y: u16) -> WindowOp {
        let base = PIECES_DS as u16 + index;
        self.piece_at(base, base + 0x12, base + 0x24, base + 0x36, x, y, 0x46)
    }

    /// `FUN_2036_0009`: build a window for content `width` (already rounded
    /// to a multiple of 8 by the caller) and `height`.
    #[must_use]
    pub fn layout(&self, width: u16, height: u16, seed: u32) -> WindowLayout {
        let mut rng = Rng(seed);
        let full_width = width.wrapping_add(0x20);
        let mut height = height;
        let mut top = 0u16;
        let bottom;
        let kind;
        if height < 0x15 {
            height = 0x14;
            bottom = 0x13;
            kind = 1;
        } else if height < 0x29 {
            height = 0x27;
            bottom = 0x26;
            kind = 2;
        } else if height < 0x65 {
            bottom = height - 1;
            kind = 3;
        } else {
            let rounded = (height.wrapping_add(0x31) / 10).wrapping_mul(10);
            height = rounded.wrapping_sub(2);
            top = 0x13;
            bottom = rounded.wrapping_sub(0x16);
            kind = 4;
        }

        let mut ops = vec![WindowOp::Fill {
            x: 0,
            y: 0,
            width: full_width,
            height,
            color: self.color(0x1f58),
        }];
        if kind > 3 {
            let half = full_width >> 1;
            let p = |sx, sy, w, h, x, y, mode| self.piece_at(sx, sy, w, h, x, y, mode);
            ops.push(p(0x5d72, 0x5d85, 0x5d96, 0x5da8, half + 1, 0, 0x46));
            ops.push(p(0x5d72, 0x5d85, 0x5d96, 0x5da8, half + 1, bottom, 0x46));
            ops.push(p(0x5d73, 0x5d84, 0x5d97, 0x5da9, half.wrapping_sub(0x45), 0, 0x47));
            ops.push(p(0x5d73, 0x5d84, 0x5d97, 0x5da9, half.wrapping_sub(0x45), bottom, 0x47));
            if full_width > 0xb8 {
                ops.push(p(0x5d73, 0x5d85, 0x5d97, 0x5da9, half + 0x46, 0, 0x47));
                ops.push(p(0x5d73, 0x5d85, 0x5d97, 0x5da9, half + 0x46, bottom, 0x47));
                ops.push(p(0x5d72, 0x5d84, 0x5d96, 0x5da8, half.wrapping_sub(0x8a), 0, 0x46));
                ops.push(p(0x5d72, 0x5d84, 0x5d96, 0x5da8, half.wrapping_sub(0x8a), bottom, 0x46));
            }
        }
        ops.push(self.piece(0, 0, 0));
        ops.push(self.piece(3, width + 10, height - 0x14));
        self.stone_area(&mut ops, &mut rng, 0x15, top, width + 10, bottom);
        if kind > 1 {
            ops.push(self.piece(2, 0, height - 0x14));
            ops.push(self.piece(1, width + 10, 0));
        }
        if kind > 2 {
            self.stone_area(&mut ops, &mut rng, 0, 0x13, 0x15, height - 0x14);
            self.stone_area(&mut ops, &mut rng, width + 10, 0x13, width + 0x1f, height - 0x14);
        }

        WindowLayout {
            width: full_width,
            height,
            content_x: 0x1f,
            content_y: (top + 5) as i16,
            ops,
        }
    }

    /// `FUN_2036_0475`: border pieces around `(x0, y0)..(x1, y1)` plus
    /// random speckles inside.
    fn stone_area(&self, ops: &mut Vec<WindowOp>, rng: &mut Rng, x0: u16, y0: u16, x1: u16, y1: u16) {
        let right = x1.wrapping_sub(10);
        let lower = y1.wrapping_sub(9);
        let mut y = y0;
        loop {
            y = y.wrapping_add(10);
            if y >= lower {
                break;
            }
            ops.push(self.piece(if rng.below(6) == 0 { 9 } else { 8 }, x0, y));
            ops.push(self.piece(if rng.below(6) == 0 { 11 } else { 10 }, right, y));
        }
        let mut x = x0.wrapping_add(10);
        while x < right {
            ops.push(self.piece(if rng.below(3) == 0 { 14 } else { 12 }, x, y0));
            ops.push(self.piece(if rng.below(3) == 0 { 15 } else { 13 }, x, lower));
            x = x.wrapping_add(11);
        }
        ops.push(self.piece(4, x0, y0));
        ops.push(self.piece(6, right, y0));
        ops.push(self.piece(5, x0, lower));
        ops.push(self.piece(7, right, lower));

        let inner_h = (lower.wrapping_sub(y0) as i16).wrapping_sub(10);
        let inner_w = (right.wrapping_sub(x0) as i16).wrapping_sub(0xb);
        if inner_h > 0 && inner_w > 0 {
            let count = (i32::from(inner_h) * i32::from(inner_w) / 100) as i16;
            for color in [self.color(0x1f57), self.color(0x1f56)] {
                for _ in 0..count {
                    let dy = rng.below(inner_h as u16);
                    let dx = rng.below(inner_w as u16);
                    ops.push(WindowOp::Fill {
                        x: (dx + x0 + 0xb) as i16,
                        y: (dy + y0 + 10) as i16,
                        width: 1,
                        height: 1,
                        color,
                    });
                }
            }
        }
    }

    /// Button frame from `FUN_1772_01ec`: three nested outlines around a
    /// label drawn at `(x, y)`, `label_width` being the widest label.
    #[must_use]
    pub fn button(&self, x: i16, y: i16, label_width: u16, highlighted: bool) -> Vec<WindowOp> {
        let w = label_width + 6;
        let face = if highlighted {
            self.color(0x1f4e)
        } else {
            self.color(0x1f4d)
        };
        let mut ops = Vec::with_capacity(12);
        outline(&mut ops, x - 4, y - 3, w, 0xc, self.color(0x1f57));
        outline(&mut ops, x - 2, y - 1, w, 0xc, self.color(0x1f56));
        outline(&mut ops, x - 3, y - 2, w, 0xc, face);
        ops
    }
}

/// `FUN_14b0_033e`: one-pixel rectangle outline.
fn outline(ops: &mut Vec<WindowOp>, x: i16, y: i16, width: u16, height: u16, color: u8) {
    let line = |x, y, width, height| WindowOp::Fill {
        x,
        y,
        width,
        height,
        color,
    };
    ops.push(line(x, y, width, 1));
    ops.push(line(x, y, 1, height));
    ops.push(line(x, y + height as i16 - 1, width, 1));
    ops.push(line(x + width as i16 - 1, y, 1, height));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tables as they appear in `EXCAL_UNPACKED.EXE`.
    fn style() -> WindowStyle {
        let mut pieces = [0u8; 72];
        let sx = [0, 22, 44, 66, 88, 88, 99, 99, 110, 110, 121, 121, 132, 132, 143, 143, 155, 0];
        let sy = [0, 0, 0, 0, 0, 10, 0, 10, 0, 10, 0, 10, 0, 10, 0, 10, 0, 0];
        let w = [22, 22, 22, 22, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 70, 70];
        let h = [20, 20, 20, 20, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 20, 20];
        for (i, table) in [sx, sy, w, h].iter().enumerate() {
            pieces[i * 18..i * 18 + 18].copy_from_slice(table);
        }
        WindowStyle {
            pieces,
            colors: [2, 8, 9, 0xe, 0x17, 4, 0xa, 0xc, 0xf, 6, 4, 5],
        }
    }

    #[test]
    fn action_menu_window_geometry() {
        // FUN_1ba8_4604: width (0xfa + 7) & 0xfff8, height 6+18+18+13+3*10.
        let layout = style().layout(256, 75, 1);
        assert_eq!((layout.width, layout.height), (288, 75));
        assert_eq!((layout.content_x, layout.content_y), (0x1f, 5));
        assert_eq!(
            layout.ops[0],
            WindowOp::Fill {
                x: 0,
                y: 0,
                width: 288,
                height: 75,
                color: 5
            }
        );
        // Top-left and bottom-right corner ornaments.
        assert!(layout.ops.contains(&WindowOp::Piece {
            src_x: 0,
            src_y: 0,
            width: 22,
            height: 20,
            x: 0,
            y: 0,
            mirror: false
        }));
        assert!(layout.ops.contains(&WindowOp::Piece {
            src_x: 66,
            src_y: 0,
            width: 22,
            height: 20,
            x: 266,
            y: 55,
            mirror: false
        }));
        // Inner frame corner pieces at (0x15, 0) and (256, 65).
        assert!(layout.ops.contains(&WindowOp::Piece {
            src_x: 88,
            src_y: 0,
            width: 11,
            height: 10,
            x: 0x15,
            y: 0,
            mirror: false
        }));
        assert!(layout.ops.contains(&WindowOp::Piece {
            src_x: 99,
            src_y: 10,
            width: 11,
            height: 10,
            x: 256,
            y: 65,
            mirror: false
        }));
    }

    #[test]
    fn height_classes() {
        let s = style();
        assert_eq!(s.layout(64, 10, 0).height, 0x14);
        assert_eq!(s.layout(64, 30, 0).height, 0x27);
        assert_eq!(s.layout(64, 0x64, 0).height, 0x64);
        let tall = s.layout(256, 120, 0);
        assert_eq!((tall.height, tall.content_y), (158, 0x13 + 5));
    }

    #[test]
    fn button_outlines() {
        let ops = style().button(34, 21, 40, false);
        assert_eq!(ops.len(), 12);
        assert_eq!(
            ops[0],
            WindowOp::Fill {
                x: 30,
                y: 18,
                width: 46,
                height: 1,
                color: 4
            }
        );
        let lit = style().button(34, 21, 40, true);
        assert!(matches!(lit[8], WindowOp::Fill { color: 8, x: 31, y: 19, .. }));
    }

    #[test]
    fn reads_tables_from_exe() {
        let Ok(exe) = std::fs::read("../../ghidra/EXCAL_UNPACKED.EXE") else {
            eprintln!("SKIP: EXCAL_UNPACKED.EXE absent");
            return;
        };
        assert_eq!(WindowStyle::from_exe(&exe).unwrap(), style());
    }
}
