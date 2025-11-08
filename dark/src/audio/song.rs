///
/// song.rs
///
/// Parser for song (SNG) files
/// Thanks to the very helpful info from zombe:
/// https://www.ttlg.com/forums/showthread.php?t=64520&s=
///
use std::io::{Read, Seek};

use rand::{distributions::WeightedIndex, prelude::Distribution, thread_rng};
use tracing::trace;

use crate::ss2_common::{read_string_with_size, read_u32};

#[derive(Debug, Clone)]
pub struct Song {
    sections: Vec<SongSection>,
}

#[derive(Debug, Clone)]
pub struct SongSection {
    #[allow(dead_code)]
    name: String,
    wav_file: String,
    options: Vec<SongSectionOption>,
}

impl SongSection {
    pub fn get_next_option(&self, maybe_cue: Option<String>) -> u32 {
        // Figure out which option to try...
        let mut section_opt = 0;
        if let Some(cue) = maybe_cue {
            let normalized_cue = cue.to_ascii_lowercase();
            for (idx, option) in self.options.iter().enumerate() {
                if option.schema.contains(&normalized_cue) {
                    section_opt = idx as u32;
                    break;
                }
            }
        }

        let option = &self.options[section_opt as usize];
        option.choose_random()
    }
}

#[derive(Debug, Clone)]
pub struct SongSectionOption {
    pub schema: String,
    pub sub_options: Vec<SubOption>,
}

impl SongSectionOption {
    pub fn choose_random(&self) -> u32 {
        let mut rng = thread_rng();
        let weights = self
            .sub_options
            .iter()
            .map(|s| s.probability)
            .collect::<Vec<u32>>();
        let weight_index = WeightedIndex::new(weights).unwrap();
        let idx = weight_index.sample(&mut rng);

        self.sub_options[idx].next_index
    }
}

#[derive(Debug, Clone)]
pub struct SubOption {
    pub next_index: u32,
    pub probability: u32,
}

#[derive(Debug, Clone)]
pub struct SongPlayContext {
    current_section: u32,
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

    pub fn play_next(
        &self,
        current_context: SongPlayContext,
        cue: Option<String>,
    ) -> (SongPlayContext, String) {
        // For the current song, check if any of the options

        let current_section = &self.sections[current_context.current_section as usize];
        let new_section = current_section.get_next_option(cue);
        (
            SongPlayContext {
                current_section: new_section,
            },
            self.sections[new_section as usize].wav_file.to_owned(),
        )
    }
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
