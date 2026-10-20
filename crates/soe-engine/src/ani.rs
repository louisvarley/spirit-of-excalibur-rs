//! ANI.RES FRML frame-bank wrapper.
//!
//! Header: `u8 format=5`, `u8 frame_count`.
//! Then `frame_count` six-byte descriptors:
//! `u16 be offset, u8 x, u8 y, u8 width, u8 height`.
//! Each frame body is a raw planar image body (no 6-byte image header):
//! `format` planes, `even(ceil(width/8))` bytes per plane row, rows first.
//!
//! Confirmed from `FUN_1316_1420`: uncompressed size is
//! `planes * sum(row_bytes(width) * height)`.

#![forbid(unsafe_code)]

use soe_video::PlanarImage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame {
    pub offset: u16,
    pub x: u8,
    pub y: u8,
    pub width: u8,
    pub height: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameBank {
    pub format: u8,
    data: Vec<u8>,
    pub frames: Vec<Frame>,
}

impl FrameBank {
    pub fn parse(data: &[u8]) -> Result<Self, String> {
        if data.len() < 2 {
            return Err("FRML record too short".into());
        }
        if data[0] != 5 {
            return Err(format!("unexpected FRML format {}", data[0]));
        }
        let count = usize::from(data[1]);
        if data.len() < 2 + count * 6 {
            return Err("FRML descriptor table truncated".into());
        }

        let mut frames = Vec::with_capacity(count);
        for i in 0..count {
            let o = 2 + i * 6;
            frames.push(Frame {
                offset: u16::from_be_bytes([data[o], data[o + 1]]),
                x: data[o + 2],
                y: data[o + 3],
                width: data[o + 4],
                height: data[o + 5],
            });
        }

        Ok(Self {
            format: data[0],
            data: data.to_vec(),
            frames,
        })
    }

    /// Decode one frame. Every FRML frame is stored as a raw planar body.
    #[must_use]
    pub fn frame_image(&self, index: usize) -> Option<PlanarImage> {
        let frame = self.frames.get(index)?;
        let width = usize::from(frame.width);
        let height = usize::from(frame.height);
        if width == 0 || height == 0 {
            return None;
        }
        let mut stride = (width + 7) / 8;
        stride += stride & 1;
        let planes = usize::from(self.format);
        let needed = planes * height * stride;
        let start = usize::from(frame.offset);
        let body = self.data.get(start..start + needed)?;

        let mut indices = vec![0u8; width * height];
        for y in 0..height {
            for plane in 0..planes {
                let row = &body[(y * planes + plane) * stride..(y * planes + plane + 1) * stride];
                for x in 0..width {
                    let bit = (row[x >> 3] >> (7 - (x & 7))) & 1;
                    indices[y * width + x] |= bit << plane;
                }
            }
        }
        Some(PlanarImage {
            width: frame.width.into(),
            height: frame.height.into(),
            planes: self.format,
            indices,
        })
    }

    /// First frame, used as the static pose.
    #[must_use]
    pub fn first_frame(&self) -> Option<PlanarImage> {
        self.frame_image(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(id: u16) -> Option<FrameBank> {
        let file = std::fs::read("../../game/RES32/ANI.RES").ok()?;
        let res = soe_res::ResFile::parse(file).ok()?;
        let rec = res.records().into_iter().find(|r| r.id == id)?;
        FrameBank::parse(&res.decode(rec).ok()?).ok()
    }

    #[test]
    fn ani_916_descriptor_table() {
        let Some(bank) = load(916) else {
            eprintln!("SKIP: ANI.RES absent");
            return;
        };
        assert_eq!(bank.format, 5);
        assert_eq!(bank.frames.len(), 4);
        assert_eq!(
            bank.frames[0],
            Frame {
                offset: 32,
                x: 1,
                y: 1,
                width: 2,
                height: 2
            }
        );
        assert_eq!(bank.frames[3].offset, 82);
        let image = bank.frame_image(0).unwrap();
        assert_eq!((image.width, image.height, image.indices.len()), (2, 2, 4));
    }

    #[test]
    fn ani_901_offsets_and_images() {
        let Some(bank) = load(901) else {
            eprintln!("SKIP: ANI.RES absent");
            return;
        };
        assert_eq!(bank.frames.len(), 3);
        assert_eq!(
            bank.frames.iter().map(|f| f.offset).collect::<Vec<_>>(),
            vec![26, 166, 306]
        );
        let image = bank.frame_image(0).unwrap();
        assert_eq!((image.width, image.height), (10, 14));
        assert!(bank.frame_image(2).is_some());
    }

    #[test]
    fn ani_700_all_frames_are_raw() {
        let Some(bank) = load(700) else {
            eprintln!("SKIP: ANI.RES absent");
            return;
        };
        assert_eq!(bank.frames.len(), 19);
        // First frame offset 0x7A, next 0x782 -> 1800 bytes = 5*60*6.
        assert_eq!(bank.frames[0].offset, 0x7a);
        assert_eq!(bank.frames[1].offset, 0x782);
        let image = bank.frame_image(0).unwrap();
        assert_eq!((image.width, image.height), (39, 60));
        assert_eq!(image.indices.len(), 39 * 60);
    }

    #[test]
    fn ani_908_keyframe_is_raw() {
        let Some(bank) = load(908) else {
            eprintln!("SKIP: ANI.RES absent");
            return;
        };
        assert_eq!(bank.frames.len(), 4);
        let image = bank.frame_image(3).unwrap();
        assert_eq!((image.width, image.height), (58, 26));
        assert!(bank.frame_image(0).is_some());
    }
}
