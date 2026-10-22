//! IFF `FORM SMUS` music from `MUSIC.RES`.
//!
//! The FORM size is unreliable; chunks are walked until the buffer ends.
//! `TRAK` chunks contain two-byte events (`sid`, `data`).

#![forbid(unsafe_code)]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteDuration {
    pub division: u8,
    pub dotted: bool,
    pub tuplet: u8,
    pub chord: bool,
    pub tie: bool,
}

impl NoteDuration {
    #[must_use]
    pub fn from_bits(bits: u8) -> Self {
        Self {
            chord: bits & 0x80 != 0,
            tie: bits & 0x40 != 0,
            tuplet: (bits >> 4) & 3,
            dotted: bits & 0x08 != 0,
            division: bits & 7,
        }
    }

    #[must_use]
    pub fn quarters(self) -> f64 {
        let base = 2f64.powi(2 - i32::from(self.division));
        let dotted = if self.dotted { 1.5 } else { 1.0 };
        let tuplet = match self.tuplet {
            1 => 2.0 / 3.0,
            2 => 4.0 / 5.0,
            3 => 6.0 / 7.0,
            _ => 1.0,
        };
        base * dotted * tuplet
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Note { tone: u8, duration: NoteDuration },
    Rest { duration: NoteDuration },
    Instrument(u8),
    TimeSignature(u8),
    KeySignature(u8),
    Dynamic(u8),
    MidiChannel(u8),
    MidiPreset(u8),
    Other { sid: u8, data: u8 },
}

impl Event {
    #[must_use]
    pub fn from_bytes(sid: u8, data: u8) -> Self {
        match sid {
            0..=127 => Self::Note {
                tone: sid,
                duration: NoteDuration::from_bits(data),
            },
            128 => Self::Rest {
                duration: NoteDuration::from_bits(data),
            },
            129 => Self::Instrument(data),
            130 => Self::TimeSignature(data),
            131 => Self::KeySignature(data),
            132 => Self::Dynamic(data),
            133 => Self::MidiChannel(data),
            134 => Self::MidiPreset(data),
            _ => Self::Other { sid, data },
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Track {
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Score {
    pub tempo: u16,
    pub volume: u8,
    pub name: String,
    pub instruments: Vec<String>,
    pub tracks: Vec<Track>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimedNote {
    pub start: f64,
    pub duration: f64,
    pub tone: u8,
    pub velocity: u8,
    pub instrument: u8,
    pub track: usize,
}

impl Score {
    pub fn parse(data: &[u8]) -> Result<Self, String> {
        if data.len() < 12 || &data[..4] != b"FORM" || &data[8..12] != b"SMUS" {
            return Err("not a FORM SMUS file".into());
        }
        let mut score = Self {
            tempo: 12_800,
            volume: 127,
            ..Self::default()
        };
        let mut instruments: Vec<(u8, String)> = Vec::new();
        let mut p = 12usize;
        while p + 8 <= data.len() {
            let id = &data[p..p + 4];
            let size =
                u32::from_be_bytes([data[p + 4], data[p + 5], data[p + 6], data[p + 7]]) as usize;
            let start = p + 8;
            let end = start.checked_add(size).ok_or("bad chunk size")?;
            if end > data.len() {
                return Err(format!(
                    "chunk {} overruns buffer",
                    String::from_utf8_lossy(id)
                ));
            }
            let body = &data[start..end];
            match id {
                // IFF is big-endian: tempo in 128ths of a quarter note per
                // minute, then volume and track count. Read little-endian
                // the tempos come out up to ~3x too fast (0xAF3E vs 0x3EAF).
                b"SHDR" if body.len() >= 4 => {
                    score.tempo = u16::from_be_bytes([body[0], body[1]]);
                    score.volume = body[2];
                }
                b"NAME" => {
                    let end = body.iter().position(|b| *b == 0).unwrap_or(body.len());
                    score.name = String::from_utf8_lossy(&body[..end]).trim().to_string();
                }
                b"INS1" => {
                    let Some(register) = body.first().copied() else {
                        p = end + (size & 1);
                        continue;
                    };
                    let raw = body.get(4..).unwrap_or(&[]);
                    let end = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
                    let name = String::from_utf8_lossy(&raw[..end]).trim().to_string();
                    if !name.is_empty() {
                        instruments.push((register, name));
                    }
                }
                b"TRAK" => {
                    let mut track = Track::default();
                    for pair in body.chunks_exact(2) {
                        track.events.push(Event::from_bytes(pair[0], pair[1]));
                    }
                    score.tracks.push(track);
                }
                _ => {}
            }
            p = end + (size & 1);
        }
        instruments.sort_by_key(|(register, _)| *register);
        let max_register = instruments.iter().map(|(r, _)| *r).max().unwrap_or(0);
        score.instruments = vec![String::new(); usize::from(max_register) + 1];
        for (register, name) in instruments {
            score.instruments[usize::from(register)] = name;
        }
        Ok(score)
    }

    #[must_use]
    pub fn seconds_per_quarter(&self) -> f64 {
        if self.tempo == 0 {
            0.5
        } else {
            60.0 * 128.0 / f64::from(self.tempo)
        }
    }

    #[must_use]
    pub fn timed_notes(&self) -> Vec<TimedNote> {
        let mut out: Vec<TimedNote> = Vec::new();
        for (track_index, track) in self.tracks.iter().enumerate() {
            let mut time = 0.0f64;
            let mut instrument = track_index as u8;
            let mut velocity = 127u8;
            let mut chord_start = 0.0;
            let mut chord_duration = 0.0f64;
            let mut in_chord = false;
            // Index in `out` of a note whose tie bit carries into the next
            // note of the same pitch on this track.
            let mut tied: Vec<(u8, usize)> = Vec::new();
            for event in &track.events {
                match *event {
                    Event::Note { tone, duration } => {
                        let start = if in_chord { chord_start } else { time };
                        let quarters = duration.quarters();
                        if in_chord {
                            chord_duration = chord_duration.max(quarters);
                        } else {
                            chord_start = start;
                            chord_duration = quarters;
                        }
                        let continued = tied
                            .iter()
                            .position(|&(pitch, _)| pitch == tone)
                            .map(|slot| tied.swap_remove(slot).1);
                        let index = match continued {
                            // A tie extends the held note instead of
                            // striking it again.
                            Some(index) => {
                                let held = &mut out[index];
                                held.duration = start + quarters - held.start;
                                index
                            }
                            None => {
                                out.push(TimedNote {
                                    start,
                                    duration: quarters,
                                    tone,
                                    velocity,
                                    instrument,
                                    track: track_index,
                                });
                                out.len() - 1
                            }
                        };
                        if duration.tie {
                            tied.push((tone, index));
                        }
                        if duration.chord {
                            in_chord = true;
                        } else {
                            in_chord = false;
                            time = chord_start + chord_duration;
                        }
                    }
                    Event::Rest { duration } => {
                        in_chord = false;
                        time += duration.quarters();
                        chord_start = time;
                    }
                    Event::Instrument(value) => instrument = value,
                    Event::Dynamic(value) => velocity = value,
                    _ => {}
                }
            }
        }
        out
    }

    #[must_use]
    pub fn duration_seconds(&self) -> f64 {
        self.timed_notes()
            .iter()
            .map(|note| note.start + note.duration)
            .fold(0.0f64, f64::max)
            * self.seconds_per_quarter()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Song {
    pub id: u16,
    pub name: String,
    pub score: Score,
}

#[derive(Debug, Clone, Default)]
pub struct MusicLibrary {
    pub songs: Vec<Song>,
}

impl MusicLibrary {
    pub fn from_res(bytes: Vec<u8>) -> Result<Self, String> {
        let file = soe_res::ResFile::parse(bytes).map_err(|e| e.to_string())?;
        let mut songs = Vec::new();
        for record in file.records() {
            let data = file.decode(record).map_err(|e| e.to_string())?;
            if let Ok(score) = Score::parse(&data) {
                songs.push(Song {
                    id: record.id,
                    name: score.name.clone(),
                    score,
                });
            }
        }
        Ok(Self { songs })
    }

    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        Self::from_res(bytes)
    }

    #[must_use]
    pub fn find(&self, name: &str) -> Option<&Song> {
        self.songs
            .iter()
            .find(|song| song.name.eq_ignore_ascii_case(name))
            .or_else(|| {
                self.songs.iter().find(|song| {
                    song.name
                        .to_ascii_lowercase()
                        .contains(&name.to_ascii_lowercase())
                })
            })
    }

    #[must_use]
    pub fn map(&self) -> Option<&Song> {
        self.find("bkmap.4t")
            .or_else(|| self.find("bkmap"))
            .or_else(|| self.songs.first())
    }
}

/// One AdLib operator's parameters in the driver's slot order
/// (`ADLIB.DVR` `FUN_0000_077b`, 14-byte slots at `DS:132B`): KSL, MULT,
/// FB, AR, SL, EG, DR, RR, TL, AM, VIB, KSR, FM. Wave select is passed as
/// 0 by the instrument loader (`drv_10`, `0000:03ea`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Operator {
    pub ksl: u8,
    pub multiple: u8,
    pub feedback: u8,
    pub attack: u8,
    pub sustain_level: u8,
    pub sustaining: bool,
    pub decay: u8,
    pub release: u8,
    pub total_level: u8,
    pub tremolo: bool,
    pub vibrato: bool,
    pub ksr: bool,
    /// `FM` param: 1 = frequency modulation. The driver writes
    /// `FB << 1 | (FM == 0)` to register `0xC0` (`FUN_0000_090e`).
    pub fm: bool,
}

impl Operator {
    fn from_words(words: &[u16]) -> Self {
        // Fields are stored as words; the driver keeps the low byte.
        let b = |index: usize| words[index] as u8;
        Self {
            ksl: b(0) & 3,
            multiple: b(1) & 15,
            feedback: b(2) & 7,
            attack: b(3) & 15,
            sustain_level: b(4) & 15,
            sustaining: b(5) != 0,
            decay: b(6) & 15,
            release: b(7) & 15,
            total_level: b(8) & 63,
            tremolo: b(9) != 0,
            vibrato: b(10) != 0,
            ksr: b(11) != 0,
            fm: b(12) != 0,
        }
    }
}

/// One instrument from `SEX.IDF`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timbre {
    pub name: String,
    /// Header byte 0: percussive voice (only `TomDrum`).
    pub percussive: bool,
    /// Header byte 1: AdLib percussion voice number.
    pub voice: u8,
    pub modulator: Operator,
    /// The carrier's `feedback` slot holds junk in the file (`0xE206`);
    /// only the modulator's feedback is used.
    pub carrier: Operator,
}

/// The AdLib instrument bank `SEX.IDF`: `u16 count`, then 96-byte records of
/// a 42-byte NUL-padded name, a header word (percussive, voice), 13 modulator
/// words and 13 carrier words, the 52-byte layout the driver's instrument
/// table uses (`drv_10`, stride 0x34).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstrumentBank {
    pub timbres: Vec<Timbre>,
}

impl InstrumentBank {
    const RECORD: usize = 96;
    const NAME: usize = 42;

    pub fn parse(data: &[u8]) -> Result<Self, String> {
        let count = usize::from(u16::from_le_bytes(
            data.get(..2)
                .ok_or("IDF too short")?
                .try_into()
                .map_err(|_| "IDF too short")?,
        ));
        let mut timbres = Vec::with_capacity(count);
        for index in 0..count {
            let start = 2 + index * Self::RECORD;
            let record = data
                .get(start..start + Self::RECORD)
                .ok_or_else(|| format!("IDF record {index} truncated"))?;
            let name_end = record[..Self::NAME]
                .iter()
                .position(|b| *b == 0)
                .unwrap_or(Self::NAME);
            let words: Vec<u16> = record[Self::NAME..]
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect();
            timbres.push(Timbre {
                name: String::from_utf8_lossy(&record[..name_end]).to_string(),
                percussive: record[Self::NAME] != 0,
                voice: record[Self::NAME + 1],
                modulator: Operator::from_words(&words[1..14]),
                carrier: Operator::from_words(&words[14..27]),
            });
        }
        Ok(Self { timbres })
    }

    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self, String> {
        Self::parse(&std::fs::read(path).map_err(|e| e.to_string())?)
    }

    /// Instrument by `INS1` name, ignoring case.
    #[must_use]
    pub fn find(&self, name: &str) -> Option<&Timbre> {
        self.timbres
            .iter()
            .find(|timbre| timbre.name.eq_ignore_ascii_case(name))
    }
}

/// Carrier total level for a note volume `0..=127`, as the driver scales it
/// (`FUN_0000_088c`): `0x3F - (vol * (0x3F - TL) * 2 + 0x7F) / 0xFE`.
#[must_use]
pub fn scaled_total_level(total_level: u8, volume: u8) -> u8 {
    let level = i32::from(0x3f - (total_level & 0x3f));
    let scaled = (i32::from(volume.min(127)) * level * 2 + 0x7f) / 0xfe;
    (0x3f - scaled) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use soe_res::ResFile;

    #[test]
    fn parses_instrument_bank() {
        let Ok(bytes) = std::fs::read("../../game/SEX.IDF") else {
            eprintln!("SKIP: SEX.IDF absent");
            return;
        };
        let bank = InstrumentBank::parse(&bytes).unwrap();
        let names: Vec<_> = bank.timbres.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "pclar", "TomDrum", "strings", "mclar", "clarinet", "horn", "tptvib", "Flute",
                "VocFlute"
            ]
        );
        let clarinet = bank.find("CLARINET").unwrap();
        assert_eq!(
            clarinet.modulator,
            Operator {
                ksl: 0,
                multiple: 2,
                feedback: 7,
                attack: 2,
                sustain_level: 0,
                sustaining: true,
                decay: 12,
                release: 3,
                total_level: 28,
                tremolo: false,
                vibrato: false,
                ksr: true,
                fm: true,
            }
        );
        assert!(clarinet.modulator.sustaining);
        assert_eq!(clarinet.carrier.attack, 2);
        assert_eq!(clarinet.carrier.total_level, 0);
        let drum = bank.find("tomdrum").unwrap();
        assert!(drum.percussive);
        assert_eq!(drum.voice, 6);
    }

    #[test]
    fn driver_volume_scaling() {
        assert_eq!(scaled_total_level(0, 127), 0);
        assert_eq!(scaled_total_level(0, 0), 63);
        assert_eq!(scaled_total_level(15, 127), 15);
        assert_eq!(scaled_total_level(15, 64), 39);
    }

    #[test]
    fn tied_notes_are_held_not_restruck() {
        // C quarter tied to C quarter, then D quarter.
        let quarter = 2;
        let score = Score {
            tracks: vec![Track {
                events: vec![
                    Event::from_bytes(60, quarter | 0x40),
                    Event::from_bytes(60, quarter),
                    Event::from_bytes(62, quarter),
                ],
            }],
            ..Score::default()
        };
        let notes = score.timed_notes();
        assert_eq!(notes.len(), 2);
        assert_eq!(
            (notes[0].tone, notes[0].start, notes[0].duration),
            (60, 0.0, 2.0)
        );
        assert_eq!((notes[1].tone, notes[1].start), (62, 2.0));
    }

    #[test]
    fn parses_gamewon() {
        let Ok(bytes) = std::fs::read("../../game/MUSIC.RES") else {
            eprintln!("SKIP: MUSIC.RES absent");
            return;
        };
        let file = ResFile::parse(bytes).unwrap();
        let record = file.records().into_iter().find(|r| r.id == 2).unwrap();
        let data = file.decode(record).unwrap();
        let score = Score::parse(&data).unwrap();
        assert_eq!(score.name, "gamewon");
        // SHDR starts 44 5B: big-endian 17499/128 = ~137 quarters per minute.
        assert_eq!(score.tempo, 0x445b);
        assert_eq!(score.tracks.len(), 5);
        assert!(score.instruments.iter().any(|name| name == "clarinet"));
        assert!(score.duration_seconds() > 0.0);

        let library =
            MusicLibrary::from_res(std::fs::read("../../game/MUSIC.RES").unwrap()).unwrap();
        assert!(library.songs.len() >= 20);
        assert!(library.map().is_some());
    }
}
