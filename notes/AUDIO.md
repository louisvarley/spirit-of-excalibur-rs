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

## Instruments (`SEX.IDF`)

`game/SEX.IDF` is the AdLib instrument bank. Layout: `u16 count` (9), then
96-byte records: 42-byte NUL-padded name, header word (byte 0 percussive,
byte 1 AdLib percussion voice; only `TomDrum` = 1/6), 13 modulator words,
13 carrier words. Operator word order is the AdLib driver's slot order
(`ADLIB.DVR` `FUN_0000_077b`): KSL, MULT, FB, AR, SL, EG, DR, RR, TL, AM,
VIB, KSR, FM. The instrument loader (`drv_10`) copies the 26 words into a
0x34-byte table and passes wave select 0. Register `0xC0` =
`FB << 1 | (FM == 0)` (`FUN_0000_090e`), so FM = 1 is FM synthesis. The
carrier's FB word is junk (`0xE206`/`0xFF`) and unused. Note volume
scales TL as `0x3F - (vol * (0x3F - TL) * 2 + 0x7F) / 0xFE`
(`FUN_0000_088c`). All 9 instruments the songs name are in the bank;
`soe-app` plays them with `soe_engine::smus::InstrumentBank`.
`TomDrum` is played melodically; AdLib rhythm mode is not modelled.

## Still open

- Per-scene song selection (map/title/combat/victory/defeat cues).
- 8 of 125 tracks (in `stnheng`, `village1`, `city1`, `city2`) play notes
  before any instrument event; the port defaults to register = track
  index. The original default is unverified.
- The SMUS sequencer is not located: segment `1237` is the generic resource
  loader that `FUN_1316_187b` uses, so the tick-level sequencer timing is
  unverified (likely in `ADLIB.DVR` or the timer ISR).
- Sound effects from the original `.IDF`/driver paths.
