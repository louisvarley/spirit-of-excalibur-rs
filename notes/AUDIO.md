# Audio

Status: **SMUS parsing and OPL2 playback implemented.**

## Music

`MUSIC.RES` records are IFF `FORM SMUS` files:

- `SHDR`: tempo and volume
- `NAME`: song name
- `INS1`: instrument register + name
- `TRAK`: two-byte SEvents

`soe-engine::smus::Score` parses chunks without trusting the FORM size and
flattens notes/rests/chords/ties. `MusicLibrary` loads all 25 songs.

The app renders the map song (`bkmap.4t`, falling back to the first song) with
`oplon` YM3812 synthesis and plays it through `cpal`. Output loops until the
next score is selected. If no audio device is available, the game stays fully
playable and silent.

## Controls

- `F2`: mute/unmute.

## Fixed 2026-10-22

- `SHDR` tempo is big-endian like the rest of IFF (128ths of a quarter note
  per minute). It was read little-endian, so music played 2-3x too fast
  (`bkmap.4t`: 0x3EAF = 125 BPM, read as 0xAF3E = 350 BPM). Tempos now range
  37-251 BPM and songs 2-87 s. `SHDR[2]` is volume, `SHDR[3]` track count.
- Tied notes (`0x40`) extend the held note instead of re-striking it.
- The render cap of 120 s that would cut and restart longer songs is now
  600 s.

## Still open

- Per-scene song selection (map/title/combat/victory/defeat cues).
- **Instruments:** `game/SEX.IDF` (866 bytes) is the instrument bank:
  `u16 count = 9`, then 96-byte records (name, then OPL operator
  parameters as words) for `pclar`, `TomDrum`, `strings`, `mclar`,
  `clarinet`, `horn`, `tptvib`, `Flute`, `VocFlute`, the names the `INS1`
  chunks use. `soe-app` still uses hand-made `oplon` patches. Decode the
  record layout (AdLib `.INS`-like operator fields) and load it.
- The SMUS sequencer is not located: segment `1237` is the generic resource
  loader that `FUN_1316_187b` uses, so the tick-level sequencer timing is
  unverified (likely in `ADLIB.DVR` or the timer ISR).
- Sound effects from the original `.IDF`/driver paths.
