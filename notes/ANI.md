# ANI.RES / FRML frame banks

Status: **fully decoded**.

```text
u8  format = 5          (planes)
u8  frame_count
frame_count * 6 bytes:
    u16 be data_offset
    u8  x
    u8  y
    u8  width
    u8  height
```

Each frame body starts at `data_offset` and is a **raw planar image body**
without a 6-byte image header:

- `format` planes per row
- `even(ceil(width/8))` bytes per plane row
- rows first, then planes within each row

The earlier `flags` column was the high byte of the big-endian offset. That is
why increasing values such as `0, 7, 14, 22` appeared: offsets were
`0x007A`, `0x0782`, `0x0EA8`, ...

Verification from `FUN_1316_1420`: uncompressed size is

```text
planes * sum(even_ceil(width/8) * height)
```

This accounts exactly for ANI#700's 40,434-byte payload and all frame
boundaries. ANI#916's four frames are at 32/52/72/82; ANI#901's at 26/166/306.

Implementation: `soe-engine::ani::FrameBank` + `frame_image(index)`.
`first_frame()` is used for the static room compositor. Scene 5 (`905`, `908`,
`910`) now decodes its animation frames as well.
