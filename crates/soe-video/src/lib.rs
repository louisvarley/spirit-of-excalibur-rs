//! Presentation boundary.
//!
//! All coordinates are logical 320x200 coordinates. Output resolution, scaling,
//! aspect correction and filtering live only in the presenter, never in game
//! logic.
//!
//! Every `Video` method is **provisional** until MCGA.DVR is reversed.

#![forbid(unsafe_code)]

use soe_assets::{Palette, ResourceId};
use thiserror::Error;

/// Original logical width.
pub const LOGICAL_WIDTH: u16 = 320;
/// Original logical height.
pub const LOGICAL_HEIGHT: u16 = 200;

/// A handle to an image resolved through the asset layer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImageHandle(pub ResourceId);

/// A handle to a palette resolved through the asset layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PaletteHandle(pub u16);

/// Logical video mode requested by game code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoMode {
    /// Original 320x200 indexed mode (MCGA/VGA).
    Mode320x200,
    /// Text mode used by setup/error paths.
    Text,
}

/// Blit flags observed in the driver boundary (`FUN_14b0_0bbf` modes: `0x40`
/// transparent colour 0, `0x01` mirrored). The driver also has an XOR mode
/// (`drv_6`); it is not modelled until a caller needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlitFlags {
    /// Palette index that is skipped, if any (colour 0 in the original).
    pub transparent: Option<u8>,
    /// Mirror the source horizontally.
    pub mirror_x: bool,
}

impl BlitFlags {
    /// Copy every pixel.
    pub const OPAQUE: Self = Self {
        transparent: None,
        mirror_x: false,
    };
    /// Skip colour 0 (driver mode `0x46`).
    pub const TRANSPARENT: Self = Self {
        transparent: Some(0),
        mirror_x: false,
    };

    /// The same flags, mirrored horizontally when `mirror_x` (mode `0x47`).
    #[must_use]
    pub const fn mirrored(self, mirror_x: bool) -> Self {
        Self {
            transparent: self.transparent,
            mirror_x,
        }
    }
}

/// A logical-coordinate rectangle. The origin may be off-screen; drawing
/// operations clip to the logical frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}

impl Rect {
    #[must_use]
    pub const fn new(x: i16, y: i16, width: u16, height: u16) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

/// Errors from the video boundary.
#[derive(Debug, Error)]
pub enum VideoError {
    #[error("frame dimensions must be {LOGICAL_WIDTH}x{LOGICAL_HEIGHT}")]
    WrongSize,
    #[error("presenter scale must be >= 1")]
    InvalidScale,
    #[error("unsupported operation in the reference renderer: {0}")]
    Unsupported(&'static str),
    #[error("image not registered with the video layer: {0}")]
    MissingImage(String),
}

/// An 8-bit indexed logical frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedFrame {
    pub width: u16,
    pub height: u16,
    pub pixels: Vec<u8>,
    pub palette: Palette,
}

impl IndexedFrame {
    /// A black 320x200 frame.
    #[must_use]
    pub fn black() -> Self {
        Self {
            width: LOGICAL_WIDTH,
            height: LOGICAL_HEIGHT,
            pixels: vec![0; usize::from(LOGICAL_WIDTH) * usize::from(LOGICAL_HEIGHT)],
            palette: Palette::black(),
        }
    }

    /// Create a frame from pixels and a palette.
    pub fn new(
        width: u16,
        height: u16,
        pixels: Vec<u8>,
        palette: Palette,
    ) -> Result<Self, VideoError> {
        if usize::from(width) * usize::from(height) != pixels.len() {
            return Err(VideoError::WrongSize);
        }
        Ok(Self {
            width,
            height,
            pixels,
            palette,
        })
    }

    /// Copy a pixel, with bounds checking.
    pub fn set_pixel(&mut self, x: u16, y: u16, color: u8) {
        if x < self.width && y < self.height {
            let index = usize::from(y) * usize::from(self.width) + usize::from(x);
            self.pixels[index] = color;
        }
    }

    /// Read a pixel.
    #[must_use]
    pub fn pixel(&self, x: u16, y: u16) -> Option<u8> {
        if x < self.width && y < self.height {
            let index = usize::from(y) * usize::from(self.width) + usize::from(x);
            Some(self.pixels[index])
        } else {
            None
        }
    }
}

/// A decoded 5-plane (or 1..8-plane) image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanarImage {
    pub width: u16,
    pub height: u16,
    pub planes: u8,
    pub indices: Vec<u8>,
}

/// Decode the big-endian planar image format used by `RES32` records.
///
/// Header: `be16 height, be16 width, be16 planes`. Body: for each row, each
/// plane's row in turn, `ceil(width/8)` bytes rounded up to an even number,
/// MSB first. Pixel index = `sum(bit(plane) << plane)`.
pub fn decode_planar(payload: &[u8]) -> Result<PlanarImage, VideoError> {
    if payload.len() < 6 {
        return Err(VideoError::WrongSize);
    }
    let height = u16::from_be_bytes([payload[0], payload[1]]);
    let width = u16::from_be_bytes([payload[2], payload[3]]);
    let planes = u16::from_be_bytes([payload[4], payload[5]]);
    if planes == 0 || planes > 8 || width == 0 || height == 0 {
        return Err(VideoError::WrongSize);
    }
    let mut stride = (usize::from(width) + 7) / 8;
    stride += stride & 1;
    let needed = 6 + usize::from(planes) * usize::from(height) * stride;
    if payload.len() < needed {
        return Err(VideoError::WrongSize);
    }
    let mut indices = vec![0u8; usize::from(width) * usize::from(height)];
    let body = &payload[6..];
    for y in 0..usize::from(height) {
        for plane in 0..usize::from(planes) {
            let row = &body[(y * usize::from(planes) + plane) * stride
                ..(y * usize::from(planes) + plane + 1) * stride];
            for x in 0..usize::from(width) {
                let bit = (row[x >> 3] >> (7 - (x & 7))) & 1;
                indices[y * usize::from(width) + x] |= bit << plane;
            }
        }
    }
    Ok(PlanarImage {
        width,
        height,
        planes: planes as u8,
        indices,
    })
}

/// A composited logical frame. The original is indexed; override images are RGBA.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogicalFrame {
    Indexed(IndexedFrame),
    Rgba {
        width: u16,
        height: u16,
        rgba: Vec<u8>,
    },
}

impl LogicalFrame {
    #[must_use]
    pub const fn dimensions(&self) -> (u16, u16) {
        match self {
            Self::Indexed(f) => (f.width, f.height),
            Self::Rgba { width, height, .. } => (*width, *height),
        }
    }
}

/// How text glyphs are coloured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextColor {
    /// The atlas's own shaded colours, as the original draws all game text
    /// (`14b0:0af9`, blit mode `0x46`, no remap).
    Atlas,
    /// Every glyph pixel in one palette index (screens without original art).
    Solid(u8),
}

/// Metrics of the game's atlas font (`DS:1E8A..`, see `notes/FONT.md`). The
/// atlas pixels stay with the `Video` implementation; this holds no pixels,
/// so glyphs or whole strings can be replaced by an override pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtlasFont {
    /// Atlas image (`TITLE.RES#200`).
    pub atlas: ImageHandle,
    /// Line advance (`DS:1E8A`).
    pub line_height: u16,
    /// Inter-character spacing (`DS:1E8B`).
    pub spacing: u8,
    /// 95 metric words for codes `0x20..=0x7E` (`DS:1E8C`).
    pub metrics: Vec<u16>,
}

/// One glyph's metric word, decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyph {
    pub src_x: u16,
    pub width: u8,
    pub left_overhang: bool,
    pub right_overhang: bool,
}

impl AtlasFont {
    /// Decoded metric for a printable code.
    #[must_use]
    pub fn glyph(&self, code: u8) -> Option<Glyph> {
        if !(0x20..=0x7e).contains(&code) {
            return None;
        }
        let metric = *self.metrics.get(usize::from(code - 0x20))?;
        Some(Glyph {
            src_x: metric & 0x3ff,
            width: (metric >> 12) as u8,
            left_overhang: metric >> 11 & 1 != 0,
            right_overhang: metric >> 10 & 1 != 0,
        })
    }

    /// Pen advance (`FUN_14b0_094f`).
    #[must_use]
    pub fn advance(&self, glyph: Glyph) -> i32 {
        i32::from(glyph.width) + i32::from(self.spacing)
            - i32::from(glyph.left_overhang)
            - i32::from(glyph.right_overhang)
    }

    /// Advance width of one line of text in logical pixels.
    #[must_use]
    pub fn text_width(&self, text: &str) -> u32 {
        text.bytes()
            .filter_map(|code| self.glyph(code))
            .map(|glyph| self.advance(glyph).max(0) as u32)
            .sum()
    }
}

/// The platform boundary. **Every method is provisional** until MCGA.DVR is
/// fully reversed. Operations take resource handles and logical rectangles,
/// never raw pixel slices, so HD assets and scaled presentation stay outside
/// game logic.
pub trait Video {
    /// Enter a logical video mode.
    fn set_mode(&mut self, mode: VideoMode) -> Result<(), VideoError>;
    /// Blit a source rect of an image handle unscaled to logical `(x, y)`,
    /// clipped to the frame (the driver never scales).
    fn blit_image(
        &mut self,
        image: &ImageHandle,
        src: Rect,
        x: i16,
        y: i16,
        flags: BlitFlags,
    ) -> Result<(), VideoError>;
    /// Fill a logical rectangle with a palette index, clipped to the frame.
    fn fill_rect(&mut self, rect: Rect, color: u8) -> Result<(), VideoError>;
    /// Copy a logical region (save-under/readback path; existence is unverified).
    fn copy_rect(&mut self, src: Rect, dst_x: u16, dst_y: u16) -> Result<(), VideoError>;
    /// Install the atlas font used by `draw_text`.
    fn set_font(&mut self, font: AtlasFont);
    /// Draw text with the installed font, pen origin at the glyph top-left;
    /// a newline starts a new line at `x`.
    fn draw_text(&mut self, x: i16, y: i16, text: &str, color: TextColor)
    -> Result<(), VideoError>;
    /// Install a palette by handle.
    fn set_palette(&mut self, palette: PaletteHandle);
    /// Apply a presentation-time fade (0 = black, 255 = normal).
    fn fade_palette(&mut self, level: u8);
    /// Present the current logical frame.
    fn present(&mut self) -> Result<(), VideoError>;
    /// Optional logical-resolution readback. Open question until MCGA.DVR is
    /// reversed; the reference renderer exposes it, hardware may not.
    fn readback(&self) -> Option<&IndexedFrame>;
    /// Read one logical pixel, if the platform can do so.
    fn read_pixel(&self, x: u16, y: u16) -> Option<u8>;
}

/// Pixel-exact 320x200 indexed software reference implementation. Image
/// pixels are registered by the asset layer and only ever indexed here.
#[derive(Debug, Clone)]
pub struct SoftwareVideo {
    frame: IndexedFrame,
    base_palette: Palette,
    images: std::collections::HashMap<ImageHandle, PlanarImage>,
    font: Option<AtlasFont>,
}

impl SoftwareVideo {
    #[must_use]
    pub fn new() -> Self {
        Self {
            frame: IndexedFrame::black(),
            base_palette: Palette::black(),
            images: std::collections::HashMap::new(),
            font: None,
        }
    }

    /// Make a decoded image available to `blit_image` under `handle`.
    pub fn register_image(&mut self, handle: ImageHandle, image: PlanarImage) {
        self.images.insert(handle, image);
    }

    /// Whether `handle` has been registered.
    #[must_use]
    pub fn has_image(&self, handle: &ImageHandle) -> bool {
        self.images.contains_key(handle)
    }

    /// Clear the frame to colour 0, keeping images, font and palette.
    pub fn clear(&mut self) {
        self.frame.pixels.fill(0);
    }

    fn plot(&mut self, x: i32, y: i32, color: u8) {
        if (0..i32::from(self.frame.width)).contains(&x)
            && (0..i32::from(self.frame.height)).contains(&y)
        {
            self.frame.set_pixel(x as u16, y as u16, color);
        }
    }

    #[must_use]
    pub fn frame(&self) -> &IndexedFrame {
        &self.frame
    }

    /// Reference-only blit of an already-decoded indexed frame.
    pub fn blit_indexed(&mut self, src: &IndexedFrame, x: u16, y: u16) -> Result<(), VideoError> {
        for sy in 0..src.height {
            for sx in 0..src.width {
                if let Some(pixel) = src.pixel(sx, sy) {
                    self.frame
                        .set_pixel(x.wrapping_add(sx), y.wrapping_add(sy), pixel);
                }
            }
        }
        Ok(())
    }

    /// Reference-only palette installation for tests.
    pub fn set_palette_data(&mut self, palette: &Palette) {
        self.base_palette = palette.clone();
        self.frame.palette = palette.clone();
    }
}

impl Default for SoftwareVideo {
    fn default() -> Self {
        Self::new()
    }
}

impl Video for SoftwareVideo {
    fn set_mode(&mut self, _mode: VideoMode) -> Result<(), VideoError> {
        Ok(())
    }

    fn blit_image(
        &mut self,
        image: &ImageHandle,
        src: Rect,
        x: i16,
        y: i16,
        flags: BlitFlags,
    ) -> Result<(), VideoError> {
        let Some(source) = self.images.get(image) else {
            return Err(VideoError::MissingImage(image.0.key()));
        };
        let source_width = i32::from(source.width);
        let source_height = i32::from(source.height);
        let mut plotted = Vec::new();
        for row in 0..i32::from(src.height) {
            let sy = i32::from(src.y) + row;
            if !(0..source_height).contains(&sy) {
                continue;
            }
            for col in 0..i32::from(src.width) {
                let sx = if flags.mirror_x {
                    i32::from(src.x) + i32::from(src.width) - 1 - col
                } else {
                    i32::from(src.x) + col
                };
                if !(0..source_width).contains(&sx) {
                    continue;
                }
                let pixel = source.indices[(sy * source_width + sx) as usize];
                if flags.transparent != Some(pixel) {
                    plotted.push((i32::from(x) + col, i32::from(y) + row, pixel));
                }
            }
        }
        for (px, py, pixel) in plotted {
            self.plot(px, py, pixel);
        }
        Ok(())
    }

    fn fill_rect(&mut self, rect: Rect, color: u8) -> Result<(), VideoError> {
        for row in 0..i32::from(rect.height) {
            for col in 0..i32::from(rect.width) {
                self.plot(i32::from(rect.x) + col, i32::from(rect.y) + row, color);
            }
        }
        Ok(())
    }

    fn copy_rect(&mut self, src: Rect, dst_x: u16, dst_y: u16) -> Result<(), VideoError> {
        let mut temp = Vec::with_capacity(usize::from(src.width) * usize::from(src.height));
        for row in 0..i32::from(src.height) {
            for col in 0..i32::from(src.width) {
                let (x, y) = (i32::from(src.x) + col, i32::from(src.y) + row);
                let pixel = if x >= 0 && y >= 0 {
                    self.frame.pixel(x as u16, y as u16).unwrap_or(0)
                } else {
                    0
                };
                temp.push(pixel);
            }
        }
        let mut i = 0;
        for y in dst_y..dst_y.saturating_add(src.height) {
            for x in dst_x..dst_x.saturating_add(src.width) {
                self.frame.set_pixel(x, y, temp[i]);
                i += 1;
            }
        }
        Ok(())
    }

    fn set_font(&mut self, font: AtlasFont) {
        self.font = Some(font);
    }

    fn draw_text(
        &mut self,
        x: i16,
        y: i16,
        text: &str,
        color: TextColor,
    ) -> Result<(), VideoError> {
        let Some(font) = self.font.as_ref() else {
            return Err(VideoError::Unsupported("draw_text before set_font"));
        };
        let Some(atlas) = self.images.get(&font.atlas) else {
            return Err(VideoError::MissingImage(font.atlas.0.key()));
        };
        let atlas_width = i32::from(atlas.width);
        let atlas_height = i32::from(atlas.height);
        let mut plotted = Vec::new();
        let mut pen = i32::from(x);
        let mut top = i32::from(y);
        for code in text.bytes() {
            if code == b'\n' {
                pen = i32::from(x);
                top += i32::from(font.line_height);
                continue;
            }
            let Some(glyph) = font.glyph(code) else {
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
                    let pixel = atlas.indices[(row * atlas_width + sx) as usize];
                    if pixel != 0 {
                        let pixel = match color {
                            TextColor::Atlas => pixel,
                            TextColor::Solid(index) => index,
                        };
                        plotted.push((start_x + col, top + row, pixel));
                    }
                }
            }
            pen += font.advance(glyph);
        }
        for (px, py, pixel) in plotted {
            self.plot(px, py, pixel);
        }
        Ok(())
    }

    fn set_palette(&mut self, _palette: PaletteHandle) {
        // Palette lookup belongs to the asset layer.
    }

    fn fade_palette(&mut self, level: u8) {
        self.frame.palette = Palette::new(
            self.base_palette
                .colors
                .iter()
                .map(|c| {
                    [
                        (u16::from(c[0]) * u16::from(level) / 255) as u8,
                        (u16::from(c[1]) * u16::from(level) / 255) as u8,
                        (u16::from(c[2]) * u16::from(level) / 255) as u8,
                    ]
                })
                .collect(),
        );
    }

    fn present(&mut self) -> Result<(), VideoError> {
        Ok(())
    }

    fn readback(&self) -> Option<&IndexedFrame> {
        Some(&self.frame)
    }

    fn read_pixel(&self, x: u16, y: u16) -> Option<u8> {
        self.frame.pixel(x, y)
    }
}

/// An RGBA output frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbaFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Pluggable scaling filter. Nearest is the reference; Scale2x/xBRZ can be added.
pub trait Upscaler {
    /// Upscale a logical frame by an integer factor.
    fn upscale(&self, src: &LogicalFrame, factor: u32) -> Result<RgbaFrame, VideoError>;
}

/// Nearest-neighbour scaler.
#[derive(Debug, Default, Clone, Copy)]
pub struct NearestUpscaler;

impl Upscaler for NearestUpscaler {
    fn upscale(&self, src: &LogicalFrame, factor: u32) -> Result<RgbaFrame, VideoError> {
        if factor < 1 {
            return Err(VideoError::InvalidScale);
        }
        let (w, h) = src.dimensions();
        let out_w = u32::from(w) * factor;
        let out_h = u32::from(h) * factor;
        let mut rgba = vec![0u8; (out_w * out_h * 4) as usize];
        for y in 0..out_h {
            for x in 0..out_w {
                let pixel = match src {
                    LogicalFrame::Indexed(f) => {
                        let sx = (x / factor) as u16;
                        let sy = (y / factor) as u16;
                        let index = f.pixel(sx, sy).unwrap_or(0) as usize;
                        let rgb = f.palette.colors.get(index).copied().unwrap_or([0, 0, 0]);
                        [rgb[0], rgb[1], rgb[2], 255]
                    }
                    LogicalFrame::Rgba { width, rgba, .. } => {
                        let sx = (x / factor).min(u32::from(*width) - 1);
                        let sy = (y / factor).min(u32::from(h) - 1);
                        let i = ((sy * u32::from(*width) + sx) * 4) as usize;
                        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
                    }
                };
                let i = ((y * out_w + x) * 4) as usize;
                rgba[i..i + 4].copy_from_slice(&pixel);
            }
        }
        Ok(RgbaFrame {
            width: out_w,
            height: out_h,
            rgba,
        })
    }
}

/// How the presenter maps 320x200 non-square pixels to a 4:3 display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AspectPolicy {
    /// Integer-scale pixels and let the frontend stretch to 4:3.
    SquarePixels,
    /// Stretch vertically to a 4:3 display (320x200 -> 320x240).
    Stretch4x3,
}

/// A scaled presenter. This is the only place that knows the output size.
pub struct ScaledPresenter {
    pub factor: u32,
    pub aspect: AspectPolicy,
    pub upscaler: Box<dyn Upscaler>,
}

impl ScaledPresenter {
    #[must_use]
    pub fn new(factor: u32, aspect: AspectPolicy, upscaler: Box<dyn Upscaler>) -> Self {
        Self {
            factor,
            aspect,
            upscaler,
        }
    }

    /// Present at `factor * logical size` (and 4:3 height for `Stretch4x3`).
    pub fn present(&self, src: &LogicalFrame) -> Result<RgbaFrame, VideoError> {
        let scaled = self.upscaler.upscale(src, self.factor)?;
        match self.aspect {
            AspectPolicy::SquarePixels => Ok(scaled),
            AspectPolicy::Stretch4x3 => {
                let target_h = scaled.width * 3 / 4;
                if target_h == scaled.height {
                    return Ok(scaled);
                }
                let mut rgba = vec![0u8; (scaled.width * target_h * 4) as usize];
                for y in 0..target_h {
                    let sy = y * scaled.height / target_h;
                    let src_start = (sy * scaled.width * 4) as usize;
                    let dst_start = (y * scaled.width * 4) as usize;
                    let row = (scaled.width * 4) as usize;
                    rgba[dst_start..dst_start + row]
                        .copy_from_slice(&scaled.rgba[src_start..src_start + row]);
                }
                Ok(RgbaFrame {
                    width: scaled.width,
                    height: target_h,
                    rgba,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_renderer_is_pixel_exact() {
        let mut video = SoftwareVideo::new();
        video.fill_rect(Rect::new(1, 1, 2, 2), 7).unwrap();
        let f = video.frame();
        assert_eq!(f.pixel(1, 1), Some(7));
        assert_eq!(f.pixel(2, 2), Some(7));
        assert_eq!(f.pixel(3, 3), Some(0));
    }

    fn handle(record: u32) -> ImageHandle {
        ImageHandle(ResourceId::new("TEST.RES", record, None::<String>))
    }

    #[test]
    fn blit_clips_skips_transparent_and_mirrors() {
        let mut video = SoftwareVideo::new();
        video.register_image(
            handle(1),
            PlanarImage {
                width: 3,
                height: 1,
                planes: 5,
                indices: vec![1, 0, 3],
            },
        );
        let src = Rect::new(0, 0, 3, 1);
        video
            .blit_image(&handle(1), src, -1, 0, BlitFlags::TRANSPARENT)
            .unwrap();
        assert_eq!(video.frame().pixel(0, 0), Some(0));
        assert_eq!(video.frame().pixel(1, 0), Some(3));
        video
            .blit_image(
                &handle(1),
                src,
                10,
                5,
                BlitFlags::TRANSPARENT.mirrored(true),
            )
            .unwrap();
        assert_eq!(video.frame().pixel(10, 5), Some(3));
        assert_eq!(video.frame().pixel(12, 5), Some(1));
        assert!(matches!(
            video.blit_image(&handle(2), src, 0, 0, BlitFlags::OPAQUE),
            Err(VideoError::MissingImage(_))
        ));
    }

    #[test]
    fn text_uses_atlas_colours_and_metrics() {
        let mut video = SoftwareVideo::new();
        // 'A' = 2 px at atlas x 0; 'B' = 1 px at x 2 with a left overhang.
        video.register_image(
            handle(200),
            PlanarImage {
                width: 3,
                height: 1,
                planes: 5,
                indices: vec![2, 4, 6],
            },
        );
        let mut metrics = vec![0u16; 95];
        metrics[usize::from(b'A' - 0x20)] = 2 << 12;
        metrics[usize::from(b'B' - 0x20)] = (1 << 12) | (1 << 11) | 2;
        let font = AtlasFont {
            atlas: handle(200),
            line_height: 10,
            spacing: 1,
            metrics,
        };
        assert_eq!(font.text_width("AB"), 3 + 1);
        video.set_font(font);
        video.draw_text(0, 0, "AB\nA", TextColor::Atlas).unwrap();
        let f = video.frame();
        assert_eq!(
            (f.pixel(0, 0), f.pixel(1, 0), f.pixel(2, 0)),
            (Some(2), Some(4), Some(6))
        );
        assert_eq!(f.pixel(0, 10), Some(2));
        video.draw_text(0, 20, "A", TextColor::Solid(9)).unwrap();
        assert_eq!(video.frame().pixel(1, 20), Some(9));
    }

    #[test]
    fn planar_decode_uses_even_stride() {
        // 3x2, 2 planes; stride = even(ceil(3/8)) = 2.
        let mut payload = Vec::new();
        payload.extend_from_slice(&2u16.to_be_bytes());
        payload.extend_from_slice(&3u16.to_be_bytes());
        payload.extend_from_slice(&2u16.to_be_bytes());
        // row 0: plane 0 = 101..., plane 1 = 010...
        payload.extend_from_slice(&[0b1010_0000, 0, 0b0100_0000, 0]);
        // row 1: all zero.
        payload.extend_from_slice(&[0, 0, 0, 0]);
        let img = decode_planar(&payload).unwrap();
        assert_eq!(img.width, 3);
        assert_eq!(img.height, 2);
        assert_eq!(&img.indices[..3], &[1, 2, 1]);
    }

    #[test]
    fn nearest_upscale_and_4x3() {
        let mut f = IndexedFrame::black();
        f.palette = Palette::new(vec![[255, 0, 0]; 256]);
        f.set_pixel(0, 0, 1);
        let presenter =
            ScaledPresenter::new(2, AspectPolicy::Stretch4x3, Box::new(NearestUpscaler));
        let out = presenter.present(&LogicalFrame::Indexed(f)).unwrap();
        assert_eq!(out.width, 640);
        assert_eq!(out.height, 480);
        assert_eq!(&out.rgba[0..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn palette_fade_is_presentation_only() {
        let mut video = SoftwareVideo::new();
        video.set_palette_data(&Palette::new(vec![[100, 50, 25]; 256]));
        video.fade_palette(128);
        assert_eq!(video.frame().palette.colors[0], [50, 25, 12]);
    }
}
