///
/// song.rs
///
/// Parser for song (SNG) files
/// Thanks to the very helpful info from zombe:
/// https://www.ttlg.com/forums/showthread.php?t=64520&s=
///
use std::io::{Read, Seek};

use rand::{Rng, distributions::WeightedIndex, prelude::Distribution};
use tracing::trace;

use crate::ss2_common::{read_string_with_size, read_u32};

#[derive(Debug, Clone)]
pub struct Song {
    sections: Vec<SongSection>,
}

#[derive(Debug, Clone)]
pub struct SongSection {
    pub name: String,
    pub wav_file: String,
    pub options: Vec<SongSectionOption>,
}

#[derive(Debug, Clone)]
pub struct SongSectionOption {
    pub schema: String,
    pub sub_options: Vec<SubOption>,
}

#[derive(Debug, Clone)]
pub struct SubOption {
    pub next_index: u32,
    pub probability: u32,
}

#[derive(Debug, Clone)]
pub struct SongPlayContext {
    pub current_section: usize,
}

impl Song {
    pub fn read<T: Read + Seek>(reader: &mut T) -> Song {
        let _unk_header = read_u32(reader);

        let song_name = read_string_with_size(reader, 9);
        let contact = read_string_with_size(reader, 27);

        trace!(
            "header: {} song_name: {} contact: {}",
            _unk_header, &song_name, &contact
        );

        let num_sections = read_u32(reader);

        trace!("sections: {}", num_sections);

        let mut sections = Vec::new();
        for _ in 0..num_sections {
            let section = read_section(reader);
            sections.push(section);
        }

        Song { sections }
    }

    ///
    /// all_wav_files
    ///
    /// Return all the wav files the song uses
    ///
    pub fn all_wav_files(&self) -> Vec<String> {
        self.sections
            .iter()
            .map(|s| s.wav_file.to_owned())
            .collect::<Vec<String>>()
    }

    ///
    /// all_schemas
    ///
    /// Return all the schemas the song understands
    ///
    pub fn all_schemas(&self) -> Vec<String> {
        self.sections
            .iter()
            .flat_map(|s| {
                s.options
                    .iter()
                    .map(|o| o.schema.to_owned())
                    .collect::<Vec<String>>()
            })
            .collect::<Vec<String>>()
    }

    pub fn start_playing(&self) -> SongPlayContext {
        SongPlayContext { current_section: 0 }
    }

    pub fn sections(&self) -> &[SongSection] {
        &self.sections
    }

    /// A useful initial event for auditioning: most songs wait in empty.wav
    /// for "theme begin"; others (e.g. song09) use a different authored event.
    pub fn start_event(&self) -> Option<String> {
        let options = &self.sections.first()?.options;
        options
            .iter()
            .find(|o| o.schema == "theme begin")
            .or_else(|| options.iter().find(|o| !o.schema.is_empty()))
            .map(|o| o.schema.clone())
    }

    /// Resolve once, returning the exact event and random branch that selected
    /// the next WAV. Playback and inspector consume the same decision.
    pub fn transition(
        &self,
        context: &SongPlayContext,
        cue: Option<&str>,
        rng: &mut impl Rng,
    ) -> Result<SongTransition, String> {
        let from = context.current_section;
        let section = self
            .sections
            .get(from)
            .ok_or("Song has no current section")?;
        let cue = cue.map(str::to_ascii_lowercase);
        let option_index = cue
            .as_ref()
            .filter(|c| !c.is_empty())
            .and_then(|cue| section.options.iter().position(|o| o.schema.contains(cue)))
            .unwrap_or(0);
        let option = section
            .options
            .get(option_index)
            .ok_or("Section has no event options")?;
        let weights = option
            .sub_options
            .iter()
            .map(|o| o.probability)
            .collect::<Vec<_>>();
        let distribution =
            WeightedIndex::new(&weights).map_err(|e| format!("Invalid branch weights: {e}"))?;
        let branch_index = distribution.sample(rng);
        let to = option.sub_options[branch_index].next_index as usize;
        if to >= self.sections.len() {
            return Err(format!("Branch targets missing section {to}"));
        }
        Ok(SongTransition {
            from,
            to,
            option_index,
            branch_index,
            cue,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SongTransition {
    pub from: usize,
    pub to: usize,
    pub option_index: usize,
    pub branch_index: usize,
    pub cue: Option<String>,
}

fn read_section<T: Read + Seek>(reader: &mut T) -> SongSection {
    let name = read_string_with_size(reader, 36);
    let _unk1 = read_u32(reader);
    let _unk2 = read_u32(reader);
    let wav_file = read_string_with_size(reader, 32);

    let num_options = read_u32(reader);

    trace!(
        "- reading section {} with wav: {}, {} options (unk1: {}, unk2: {}):",
        name, wav_file, num_options, _unk1, _unk2
    );

    let mut options = Vec::new();
    for _ in 0..num_options {
        let option = read_section_option(reader);
        options.push(option);
    }

    SongSection {
        name,
        wav_file,
        options,
    }
}

fn read_section_option<T: Read + Seek>(reader: &mut T) -> SongSectionOption {
    let schema = read_string_with_size(reader, 36);
    let sub_option_count = read_u32(reader);

    trace!(
        "-- reading section option - schema: {}, sub options: {}",
        schema, sub_option_count,
    );

    let mut sub_options = Vec::new();
    for _ in 0..sub_option_count {
        sub_options.push(read_sub_option(reader));
    }
    SongSectionOption {
        schema: schema.to_ascii_lowercase(),
        sub_options,
    }
}

fn read_sub_option<T: Read + Seek>(reader: &mut T) -> SubOption {
    let next_index = read_u32(reader);
    let probability = read_u32(reader);
    trace!(
        "--- reading sub option - next_index: {}, probability: {}",
        next_index, probability,
    );
    SubOption {
        next_index,
        probability,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{SeedableRng, rngs::StdRng};

    fn song() -> Song {
        Song {
            sections: vec![
                SongSection {
                    name: "empty".into(),
                    wav_file: "empty.wav".into(),
                    options: vec![
                        SongSectionOption {
                            schema: "".into(),
                            sub_options: vec![SubOption {
                                next_index: 0,
                                probability: 100,
                            }],
                        },
                        SongSectionOption {
                            schema: "theme begin".into(),
                            sub_options: vec![
                                SubOption {
                                    next_index: 0,
                                    probability: 0,
                                },
                                SubOption {
                                    next_index: 1,
                                    probability: 25,
                                },
                                SubOption {
                                    next_index: 2,
                                    probability: 75,
                                },
                            ],
                        },
                    ],
                },
                SongSection {
                    name: "a".into(),
                    wav_file: "a.wav".into(),
                    options: vec![],
                },
                SongSection {
                    name: "b".into(),
                    wav_file: "b.wav".into(),
                    options: vec![],
                },
            ],
        }
    }

    #[test]
    fn events_select_an_option_and_unmatched_events_use_default() {
        let song = song();
        let mut rng = StdRng::seed_from_u64(7);
        for cue in [None, Some(""), Some("unknown")] {
            let t = song
                .transition(&song.start_playing(), cue, &mut rng)
                .unwrap();
            assert_eq!((t.from, t.to, t.option_index), (0, 0, 0));
        }
        for cue in ["THEME BEGIN", "Begin"] {
            let t = song
                .transition(&song.start_playing(), Some(cue), &mut rng)
                .unwrap();
            assert_eq!(t.option_index, 1);
            assert_ne!(t.to, 0);
        }
        assert_eq!(song.start_event().as_deref(), Some("theme begin"));
    }

    #[test]
    fn weighted_branches_report_the_actual_destination_and_never_choose_zero() {
        let song = song();
        let mut rng = StdRng::seed_from_u64(23);
        let mut counts = [0; 3];
        for _ in 0..10000 {
            let t = song
                .transition(&song.start_playing(), Some("begin"), &mut rng)
                .unwrap();
            assert_eq!(t.to, t.branch_index);
            counts[t.to] += 1;
        }
        assert_eq!(counts[0], 0);
        assert!((2300..2700).contains(&counts[1]), "{counts:?}");
        assert!((7300..7700).contains(&counts[2]), "{counts:?}");
    }

    #[test]
    fn invalid_sections_weights_and_targets_report_errors() {
        let mut song = song();
        let mut rng = StdRng::seed_from_u64(1);
        assert!(
            song.transition(
                &SongPlayContext {
                    current_section: 30
                },
                None,
                &mut rng
            )
            .is_err()
        );
        assert!(
            song.transition(&SongPlayContext { current_section: 1 }, None, &mut rng)
                .is_err()
        );
        song.sections[0].options[0].sub_options[0].probability = 0;
        assert!(
            song.transition(&song.start_playing(), None, &mut rng)
                .is_err()
        );
        song.sections[0].options[0].sub_options[0] = SubOption {
            next_index: 100,
            probability: 1,
        };
        assert!(
            song.transition(&song.start_playing(), None, &mut rng)
                .is_err()
        );
    }
}
