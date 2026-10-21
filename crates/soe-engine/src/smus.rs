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
                b"SHDR" if body.len() >= 4 => {
                    score.tempo = u16::from_le_bytes([body[0], body[1]]);
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
        let mut out = Vec::new();
        for (track_index, track) in self.tracks.iter().enumerate() {
            let mut time = 0.0f64;
            let mut instrument = track_index as u8;
            let mut velocity = 127u8;
            let mut chord_start = 0.0;
            let mut chord_duration = 0.0f64;
            let mut in_chord = false;
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
                        out.push(TimedNote {
                            start,
                            duration: quarters,
                            tone,
                            velocity,
                            instrument,
                            track: track_index,
                        });
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

#[cfg(test)]
mod tests {
    use super::*;
    use soe_res::ResFile;

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
        assert_eq!(score.tracks.len(), 5);
        assert!(score.instruments.iter().any(|name| name == "clarinet"));
        assert!(score.duration_seconds() > 0.0);

        let library =
            MusicLibrary::from_res(std::fs::read("../../game/MUSIC.RES").unwrap()).unwrap();
        assert!(library.songs.len() >= 20);
        assert!(library.map().is_some());
    }
}
