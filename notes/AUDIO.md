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

## Still open

- Per-scene song selection (map/title/combat/victory/defeat cues).
- Driver-exact ADLIB instrument tables and SMUS sequencer timing.
- Sound effects from the original `.IDF`/driver paths.
