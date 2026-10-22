//! Screen drawing for the app, through the `soe-video` `Video` trait.
//!
//! `Gfx` wraps the reference `SoftwareVideo` with the two registered UI images
//! (the `TITLE.RES#200` font atlas and the `TITLE.RES#300` window sheet) and
//! the atlas font metrics. Draw code here and in `main.rs` only issues
//! `Video` calls with logical coordinates and image handles; it never touches
//! pixels. Frames the engine has already composed (room, regional map, duel)
//! enter once through `SoftwareVideo::blit_indexed`, the documented
//! readback path, until those composers move behind `Video` too.

use soe_assets::ResourceId;
use soe_engine::session::Session;
use soe_engine::window::WindowOp;
use soe_video::{
    AtlasFont, BlitFlags, ImageHandle, IndexedFrame, PlanarImage, Rect, SoftwareVideo, TextColor,
    Video,
};

/// Handle of the window art sheet (`TITLE.RES#300`).
fn sheet_handle() -> ImageHandle {
    ImageHandle(ResourceId::new(
        soe_engine::window::SHEET_FILE,
        u32::from(soe_engine::window::SHEET_ID),
        None::<String>,
    ))
}

/// Handle of the font atlas (`TITLE.RES#200`).
fn atlas_handle() -> ImageHandle {
    ImageHandle(ResourceId::new("TITLE.RES", 200, None::<String>))
}

/// UI images and font metrics loaded once at start-up.
#[derive(Clone, Default)]
pub struct UiAssets {
    atlas: Option<(PlanarImage, AtlasFont)>,
    sheet: Option<PlanarImage>,
}

impl UiAssets {
    /// `atlas` is `TITLE.RES#200`; metrics are `(line_height, spacing,
    /// words)` from `DS:1E8A..`; `sheet` is `TITLE.RES#300`.
    #[must_use]
    pub fn new(
        atlas: Option<PlanarImage>,
        metrics: Option<(u8, u8, Vec<u16>)>,
        sheet: Option<PlanarImage>,
    ) -> Self {
        let atlas = atlas
            .zip(metrics)
            .map(|(image, (line_height, spacing, metrics))| {
                let font = AtlasFont {
                    atlas: atlas_handle(),
                    line_height: u16::from(line_height),
                    spacing,
                    metrics,
                    // Matches `BitmapFont`'s solid-colour rule.
                    highlight: Some(6),
                };
                (image, font)
            });
        Self { atlas, sheet }
    }
}

/// One frame's drawing context.
pub struct Gfx {
    video: SoftwareVideo,
    font: Option<AtlasFont>,
    atlas_height: i32,
    has_sheet: bool,
}

impl Gfx {
    /// Start a frame from an engine-composed base frame.
    #[must_use]
    pub fn new(assets: &UiAssets, base: &IndexedFrame) -> Self {
        let mut video = SoftwareVideo::new();
        let mut font = None;
        let mut atlas_height = 0;
        if let Some((image, metrics)) = &assets.atlas {
            atlas_height = i32::from(image.height);
            video.register_image(atlas_handle(), image.clone());
            video.set_font(metrics.clone());
            font = Some(metrics.clone());
        }
        if let Some(sheet) = &assets.sheet {
            video.register_image(sheet_handle(), sheet.clone());
        }
        // Readback path: the engine composed this frame already.
        let _ = video.blit_indexed(base, 0, 0);
        Self {
            video,
            font,
            atlas_height,
            has_sheet: assets.sheet.is_some(),
        }
    }

    /// The finished logical frame.
    #[must_use]
    pub fn into_frame(self) -> IndexedFrame {
        self.video.frame().clone()
    }

    #[must_use]
    pub fn has_font(&self) -> bool {
        self.font.is_some()
    }

    #[must_use]
    pub fn has_sheet(&self) -> bool {
        self.has_sheet
    }

    /// Height of the font atlas in rows.
    #[must_use]
    pub fn atlas_height(&self) -> i32 {
        self.atlas_height
    }

    /// Draw text; `None` keeps the atlas colours, `Some(index)` is solid.
    pub fn text(&mut self, x: i32, y: i32, text: &str, color: Option<u8>) {
        if self.font.is_none() {
            return;
        }
        let color = color.map_or(TextColor::Atlas, TextColor::Solid);
        let _ = self.video.draw_text(x as i16, y as i16, text, color);
    }

    #[must_use]
    pub fn text_width(&self, text: &str) -> u32 {
        self.font.as_ref().map_or(0, |font| font.text_width(text))
    }

    pub fn fill(
        &mut self,
        x: impl Into<i32>,
        y: impl Into<i32>,
        width: impl Into<i32>,
        height: impl Into<i32>,
        color: u8,
    ) {
        let (x, y, width, height) = (x.into(), y.into(), width.into(), height.into());
        if width > 0 && height > 0 {
            let rect = Rect::new(x as i16, y as i16, width as u16, height as u16);
            let _ = self.video.fill_rect(rect, color);
        }
    }

    /// Execute window drawing operations at `(x, y)` against the sheet.
    pub fn window_ops(&mut self, x: i32, y: i32, ops: &[WindowOp]) {
        let sheet = sheet_handle();
        for op in ops {
            match *op {
                WindowOp::Fill {
                    x: fx,
                    y: fy,
                    width,
                    height,
                    color,
                } => {
                    let rect = Rect::new(
                        (x + i32::from(fx)) as i16,
                        (y + i32::from(fy)) as i16,
                        width,
                        height,
                    );
                    let _ = self.video.fill_rect(rect, color);
                }
                WindowOp::Piece {
                    src_x,
                    src_y,
                    width,
                    height,
                    x: px,
                    y: py,
                    mirror,
                } => {
                    if !self.has_sheet {
                        continue;
                    }
                    // Opaque, as the previous direct blitter was. The
                    // original's mode 0x46 skips colour 0 (open question:
                    // whether any sheet piece relies on that).
                    let src = Rect::new(src_x as i16, src_y as i16, width, height);
                    let _ = self.video.blit_image(
                        &sheet,
                        src,
                        (x + i32::from(px)) as i16,
                        (y + i32::from(py)) as i16,
                        BlitFlags::OPAQUE.mirrored(mirror),
                    );
                }
            }
        }
    }

    /// Stone dialog window (`FUN_2036_0009`) at `(x, y)` covering roughly
    /// `width` x `height`; returns the content origin in frame space.
    pub fn panel(
        &mut self,
        session: &Session,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> (i32, i32) {
        let Some(style) = session.window_style() else {
            self.fill(x, y, width, height, 5);
            return (x + 0x1f, y + 5);
        };
        // FUN_1772_000e passes `(width + 7) & 0xfff8` for the inner width;
        // the builder adds 0x20 for the two side columns.
        let inner = ((width - 0x20).max(8) as u16 + 7) & 0xfff8;
        let layout = style.layout(inner, height as u16, (x as u32) << 16 | y as u32);
        self.window_ops(x, y, &layout.ops);
        (
            x + i32::from(layout.content_x),
            y + i32::from(layout.content_y),
        )
    }
}
