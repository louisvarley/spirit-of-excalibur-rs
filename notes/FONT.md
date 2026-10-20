# Game text font

Status: **atlas and metrics located and implemented**.

`FUN_14b0_0af9` is the game's character blitter. It does **not** use MCGA
entry 2's variable-width driver font. It blits from a font atlas:

- far pointer to the atlas: `DS:01BA` / `DS:01BC`
- glyph height: `DS:1E8A` = 10
- inter-character spacing: `DS:1E8B` = 1
- 95 metric words for codes `0x20..=0x7E`: `DS:1E8C`

Metric word:

```text
src_x          = word & 0x03FF
right_overhang = (word >> 10) & 1
left_overhang  = (word >> 11) & 1
width          = word >> 12
advance        = width + spacing - left_overhang - right_overhang
```

The atlas is `TITLE.RES#200`: 301x9, 5 planes. The ninth atlas row is present;
the renderer uses height 10, so the final row is blank/clipped. Palette index 0
is transparent.

Implemented in `soe-engine::font::BitmapFont`:
- `metrics_from_excal()`
- `glyph()`
- `draw_text(frame, x, y, text, color)`

The MCGA driver also exposes a separate variable-width 1bpp/mask font through
`drv_2`/`FUN_0000_07b4`; that path is retained in `soe-engine::font::Font` but
is not used by the message formatter.

Still open: exact handling of the tenth row, colour/OR blit mode 0x46, and
whether any screens use the driver font rather than the atlas.

The live atlas in DOSBox-X memory is an 8-bit linear buffer with a 304-byte
row stride (301 pixels + 3 padding bytes), confirmed from save-state.sav.

CLI: soe font dump <title.res> <excal.exe> <text> <out.png>. 
Newlines reset the pen and advance by atlas height + 1.
