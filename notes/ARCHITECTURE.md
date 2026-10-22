# Spirit of Excalibur — Rust architecture

Status: foundation only. No game logic yet. This file records the design rules
that the later implementation must follow, with rationale, plus open questions.

## 1. Game logic lives in logical coordinates only

**Rule:** gameplay, scripts, scene state, collision, UI layout and animation all
use the original 320x200 coordinate space. Game code must never see the output
resolution or scale factor.

**Rationale:** the original is a 320x200 indexed-mode game. Keeping gameplay in
that space makes the port comparable to the original and lets HD/upscaled
presentation change independently. A single hard-coded output resolution in
game logic would make HD asset packs and alternative scalers invasive.

## 2. All image access goes through resource IDs

**Rule:** an image is addressed by `ResourceId { file, record_index, name }`,
never by a raw buffer. `AssetSource` resolves an ID. Layering is:
original `.RES` source first, then zero or more override packs. Override packs
are directories with a TOML manifest containing `scale` and a mapping from
resource ID to a PNG. A missing override falls back to the original asset.

**Rationale:** this is the seam that makes HD packs possible without touching
game logic. Resource IDs are stable, human-readable and independent of storage
format. Override packs can be added/removed without recompiling.

## 3. Indexed originals, RGBA overrides, compositor at scale S

**Rule:** original images stay 8-bit indexed plus a palette. Palette effects
(fade, cycling, per-region swaps) are presentation-time operations. Override
images are RGBA. The compositor supports both at integer output scale S >= 1
(fractional scale later). Define explicitly how palette effects apply to RGBA
overrides (mask/LUT/shader), or leave it as an open question where unresolved.

**Rationale:** preserving indexed originals keeps palette animation faithful and
cheap; forcing early RGBA conversion would lose the original semantics. RGBA
overrides let artists supply full-colour HD art. The compositor must not assume
one representation.

## 4. `Video` is the platform boundary

**Rule:** `Video` mirrors the original MCGA.DVR driver surface. Initial methods
(provisional): blit image, fill rect, text, set palette, fade palette, present.
Every method is marked provisional until MCGA.DVR is reversed. Provide:
(a) a pixel-exact 320x200 indexed software reference implementation for tests;
(b) a scaled presenter that renders at S x logical size with a pluggable
`Upscaler` trait (nearest first, Scale2x/xBRZ later) and 4:3 aspect-correct
display, because 320x200 was shown with non-square pixels on original hardware.

**Rationale:** the driver boundary keeps DOS-specific behaviour out of game
logic and makes headless tests possible. A reference indexed renderer gives a
pixel-exact oracle. The scaled presenter is the only place that knows about
output size, aspect ratio and filtering.

**Current state (2026-10-22):** `soe-app` draws every screen through
`soe-app/src/gfx.rs` (`Gfx` over `SoftwareVideo`). UI art is registered by
handle (`TITLE.RES#200` font atlas, `TITLE.RES#300` window sheet), and text
goes through `Video::draw_text` with an `AtlasFont` that holds metrics only
(`TextColor::Atlas` = original colours, `Solid` for port-only screens).
Window scaling is `soe_video::present_nearest_xrgb`. Still outside `Video`:
the engine composers that build whole frames (`room::compose_scene`,
`WorldMap::render`/`draw_entity`, the duel frame) and `Session::draw_text`.
Their output enters `Gfx` once through `SoftwareVideo::blit_indexed`, the
readback path. Moving them behind handles is the next step and belongs to
whoever owns those modules.

## 5. Screen readback risk

**Rule:** if the original reads pixels back from the screen (save-under menus,
screen copies), the compositor needs a readback path at logical resolution.
Treat this as an open question until MCGA.DVR is reversed.

**Rationale:** a pure forward-only compositor breaks if game code depends on
reading the framebuffer. The readback path must stay logical-resolution even
when presenting at S x scale.

## 6. Sound and Input are driver boundaries too

**Rule:** define `Sound` and `Input` traits mirroring the eventual driver
boundary now, with no implementation. They use logical/game-level events, not
platform APIs.

**Rationale:** keeps the same isolation as `Video`; drivers can be replaced or
stubbed for tests. No audio/input implementation is part of this foundation.

## 7. 16-bit semantics are explicit

**Rule:** `u16`/`i16` with wrapping operations, `u32`/`i32` for DX:AX values,
and explicit signed/unsigned comparisons. No reliance on Rust overflow panics
or accidental widening.

**Rationale:** the original is 16-bit real-mode Turbo C. Wrapping, SAR vs SHR,
and signed vs unsigned compares are observable behaviour.

## Open questions

- Does MCGA.DVR expose screen readback (save-under/menu copies)? If so, at what
  logical resolution and with what palette state?
- Exact MCGA.DVR operations and register/port usage: mode set, page flip,
  palette programming, wait-for-retrace, text rendering, mouse cursor.
- How should palette effects apply to RGBA override images: a per-asset mask,
  a palette LUT texture, a shader, or disable fades for overrides?
- Override-pack key syntax: is `file:record:name` sufficient, or do some
  resources need a sub-record/name hierarchy?
- Fractional scaling and aspect handling: stretch 320x200 to 4:3, or letterbox
  integer-scaled output? The presenter should expose both policies.
- Are there resources whose record layout differs from the two-u32 header
  (SCENES.RES is known not to fit)? Record directory layout still needs
  reversing; see `notes/ANALYSIS.md` and the RES reader caveats.
- Which parts of the original runtime (`1057:0000` LZSS decoder, resource
  manager) are safe to replace with Rust without changing observable data?
- Sound/Input driver boundaries are placeholders until the DVR files and
  interrupt/port usage are reversed.
