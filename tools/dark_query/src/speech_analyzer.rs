use std::collections::{BTreeMap, BTreeSet};

use anyhow::{anyhow, bail, Result};
use dark::{
    gamesys::{self, SpeechDB, Voice},
    tag_database::{TagQuery, TagQueryItem},
};
use tracing::info;

use crate::data_loader::load_gamesys;

const MAX_ENUM_VALUES_PREVIEW: usize = 8;

pub struct SpeechAnalyzer {
    gamesys: gamesys::Gamesys,
}

impl SpeechAnalyzer {
    pub fn new() -> Result<Self> {
        let gamesys = load_gamesys()?;
        Ok(Self { gamesys })
    }

    pub fn list_voices(&self) -> Result<()> {
        let speech_db = self.gamesys.speech_db();
        let voice_count = speech_db.voices.len();

        println!("Available voices: {}", voice_count);
        for (idx, voice) in speech_db.voices.iter().enumerate() {
            let stats = self.voice_stats(idx, voice);
            let label = self.voice_label(idx);

            let mut line = format!(
                "  [{}] concepts: {}, schemas: {}, samples: {}",
                label, stats.concept_count, stats.schema_count, stats.sample_count
            );

            if let Some(hint) = stats.sample_hint {
                line.push_str(&format!(" | example {}", hint));
            }

            println!("{}", line);
        }

        println!();
        println!("Usage examples:");
        println!("  dark_query speech            # list voices");
        println!("  dark_query speech 0          # show tags for voice 0");
        println!("  dark_query speech 0 +concept:spotplayer +alertlevel:two");

        Ok(())
    }

    pub fn describe_voice(&self, voice_identifier: &str, tags: &[String]) -> Result<()> {
        let voice_idx = self.parse_voice_identifier(voice_identifier)?;
        let speech_db = self.gamesys.speech_db();
        let voice = speech_db
            .voices
            .get(voice_idx)
            .ok_or_else(|| anyhow!("Voice {} not found", voice_identifier))?;

        let stats = self.voice_stats(voice_idx, voice);
        println!(
            "=== Voice {} ===",
            self.voice_label_with_hint(voice_idx, &stats)
        );

        if tags.is_empty() {
            self.print_voice_tags(voice_idx, voice, speech_db, &stats)?;
        } else {
            self.query_voice_with_tags(voice_idx, voice, speech_db, tags)?;
        }

        Ok(())
    }

    fn print_voice_tags(
        &self,
        voice_idx: usize,
        voice: &Voice,
        speech_db: &SpeechDB,
        stats: &VoiceStats,
    ) -> Result<()> {
        println!(
            "Concept count: {} | Schema count: {} | Sample count: {}",
            stats.concept_count, stats.schema_count, stats.sample_count
        );
        if let Some(hint) = &stats.sample_hint {
            println!("Example clip: {}", hint);
        }

        let concept_entries = speech_db.concept_map.entries();
        if !concept_entries.is_empty() {
            let concept_names: Vec<String> =
                concept_entries.into_iter().map(|(_, name)| name).collect();
            println!(
                "Concepts ({}): {}",
                concept_names.len(),
                concept_names.join(", ")
            );
        }

        let tag_summary = self.aggregate_voice_tags(voice, speech_db);
        if tag_summary.is_empty() {
            println!("No tag metadata found for this voice.");
            return Ok(());
        }

        println!();
        println!("Available tags ({} total):", tag_summary.len());

        let mut entries: Vec<(String, u32, TagSummary)> = tag_summary
            .into_iter()
            .map(|(tag_id, summary)| {
                let name = speech_db
                    .tag_map
                    .get_name(tag_id)
                    .cloned()
                    .unwrap_or_else(|| format!("#{}", tag_id));
                (name, tag_id, summary)
            })
            .collect();

        entries.sort_by(|a, b| a.0.cmp(&b.0));

        for (tag_name, tag_id, summary) in entries {
            println!(
                "  +{} (id {}, seen in {} concept{}) {}",
                tag_name,
                tag_id,
                summary.occurrences,
                if summary.occurrences == 1 { "" } else { "s" },
                summary.describe()
            );
        }

        println!();
        println!("Query examples:");
        println!(
            "  dark_query speech {} +concept:spotplayer +alertlevel:two",
            voice_idx
        );
        println!(
            "  dark_query speech {} +concept:comattack +stance:aggressive",
            voice_idx
        );

        Ok(())
    }

    fn query_voice_with_tags(
        &self,
        voice_idx: usize,
        voice: &Voice,
        speech_db: &SpeechDB,
        raw_tags: &[String],
    ) -> Result<()> {
        info!(
            "Querying speech DB for voice {} with tags {:?}",
            voice_idx, raw_tags
        );

        let parsed = self.parse_tags(raw_tags, speech_db)?;

        if parsed.items.is_empty() {
            println!("No tag filters provided after normalization; listing all clips.");
        } else {
            println!("Using {} tag filter(s).", parsed.items.len());
        }

        let concept_indices: Vec<usize> = if let Some(concept_idx) = parsed.concept {
            if concept_idx >= voice.tag_maps.len() {
                bail!(
                    "Concept index {} is out of range for this voice ({} concepts available)",
                    concept_idx,
                    voice.tag_maps.len()
                );
            }
            vec![concept_idx]
        } else {
            (0..voice.tag_maps.len()).collect()
        };

        let mut found_any = false;
        for concept_idx in concept_indices {
            let concept_name = speech_db
                .concept_map
                .get_name(concept_idx as u32)
                .cloned()
                .unwrap_or_else(|| format!("#{}", concept_idx));

            let tag_db = &voice.tag_maps[concept_idx];
            let schema_candidates = if parsed.items.is_empty() {
                tag_db.collect_all_data_ids()
            } else {
                let query = TagQuery::from_items(parsed.items.clone());
                tag_db.query_match_all(&query)
            };

            let schema_ids: BTreeSet<i32> = schema_candidates.into_iter().collect();
            if schema_ids.is_empty() {
                continue;
            }

            found_any = true;
            println!(
                "\nConcept: {} (index {}) — {} schema{}",
                concept_name,
                concept_idx,
                schema_ids.len(),
                if schema_ids.len() == 1 { "" } else { "s" }
            );

            for schema_id in schema_ids {
                if let Some(samples) = self.gamesys.sound_schema().id_to_samples.get(&schema_id) {
                    println!(
                        "  Schema {}: {} sample{}",
                        schema_id,
                        samples.len(),
                        if samples.len() == 1 { "" } else { "s" }
                    );

                    for sample in samples {
                        println!(
                            "    - {} (freq {})",
                            sample.sample_name.replace('\\', "/"),
                            sample.frequency
                        );
                    }
                } else {
                    println!("  Schema {}: [no sample mapping found]", schema_id);
                }
            }
        }

        if !found_any {
            println!("No matching speech found for the provided tags.");
        }

        Ok(())
    }

    fn parse_voice_identifier(&self, voice: &str) -> Result<usize> {
        let trimmed = voice.trim();
        if trimmed.is_empty() {
            bail!("Voice identifier cannot be empty");
        }

        let speech_db = self.gamesys.speech_db();
        let voice_count = speech_db.voices.len();

        if let Ok(idx) = trimmed.parse::<usize>() {
            if idx < voice_count {
                return Ok(idx);
            }
            bail!(
                "Voice index {} out of range. Expected 0..{}",
                idx,
                voice_count.saturating_sub(1)
            );
        }

        let lowered = trimmed.to_ascii_lowercase();
        if let Some(rest) = lowered.strip_prefix("voice") {
            if let Ok(idx) = rest.parse::<usize>() {
                if idx < voice_count {
                    return Ok(idx);
                }
                bail!(
                    "Voice index {} out of range. Expected 0..{}",
                    idx,
                    voice_count.saturating_sub(1)
                );
            }
        }

        bail!(
            "Unknown voice identifier '{}'. Use an index like 0 or voice0.",
            voice
        );
    }

    fn parse_tags(&self, tags: &[String], speech_db: &SpeechDB) -> Result<ParsedSpeechQuery> {
        let mut concept: Option<usize> = None;
        let mut items: Vec<TagQueryItem> = Vec::new();

        for raw_tag in tags {
            let trimmed = raw_tag.trim();
            if trimmed.is_empty() {
                continue;
            }

            let mut normalized = trimmed.trim_start_matches('+').to_ascii_lowercase();

            let mut optional = false;
            if normalized.ends_with('?') {
                optional = true;
                normalized.pop();
            }

            let (key_part, value_part) = if let Some(pos) = normalized.find('=') {
                (
                    normalized[..pos].to_string(),
                    Some(normalized[pos + 1..].to_string()),
                )
            } else if let Some(pos) = normalized.find(':') {
                (
                    normalized[..pos].to_string(),
                    Some(normalized[pos + 1..].to_string()),
                )
            } else {
                (normalized.clone(), None)
            };

            if key_part.is_empty() {
                bail!("Invalid tag '{}': missing key", raw_tag);
            }

            if key_part == "concept" {
                let concept_name = value_part
                    .as_ref()
                    .ok_or_else(|| anyhow!("Concept tag must include a value"))?;

                let concept_index = speech_db
                    .concept_map
                    .get_index(concept_name)
                    .ok_or_else(|| anyhow!("Unknown concept '{}'", concept_name))?;

                if concept.is_some() && concept.unwrap() != concept_index as usize {
                    bail!(
                        "Multiple concept values specified; only one concept may be used per query"
                    );
                }

                concept = Some(concept_index as usize);
                continue;
            }

            let tag_id = speech_db
                .tag_map
                .get_index(&key_part)
                .ok_or_else(|| anyhow!("Unknown tag '{}'", key_part))?;

            if let Some(value) = value_part {
                if let Some(enum_index) = speech_db.value_map.get_index(&value) {
                    if enum_index > u8::MAX as u32 {
                        bail!("Value '{}' is out of range for tag '{}'", value, key_part);
                    }

                    items.push(TagQueryItem::KeyWithEnumValue(
                        tag_id,
                        enum_index as u8,
                        optional,
                    ));
                } else if let Ok(int_value) = value.parse::<i32>() {
                    items.push(TagQueryItem::KeyWithIntValue(tag_id, int_value, optional));
                } else {
                    bail!(
                        "Value '{}' for tag '{}' was not recognized as enum or integer",
                        value,
                        key_part
                    );
                }
            } else {
                items.push(TagQueryItem::Key(tag_id, optional));
            }
        }

        Ok(ParsedSpeechQuery { concept, items })
    }

    fn aggregate_voice_tags(
        &self,
        voice: &Voice,
        speech_db: &SpeechDB,
    ) -> BTreeMap<u32, TagSummary> {
        let mut summary = BTreeMap::new();
        for tag_db in &voice.tag_maps {
            for key in tag_db.collect_all_keys() {
                let entry = summary
                    .entry(key.key_type)
                    .or_insert_with(TagSummary::default);
                entry.occurrences += 1;

                if !key.enum_values.is_empty() {
                    for enum_value in &key.enum_values {
                        let display = speech_db
                            .value_map
                            .get_name(*enum_value as u32)
                            .cloned()
                            .unwrap_or_else(|| enum_value.to_string());
                        entry.enum_values.insert(display);
                    }
                } else {
                    entry.add_numeric_range(key.min, key.max);
                }
            }
        }
        summary
    }

    fn voice_stats(&self, voice_idx: usize, voice: &Voice) -> VoiceStats {
        let schema_ids = self.collect_voice_schema_ids(voice);
        let sample_count: usize = schema_ids
            .iter()
            .map(|schema_id| {
                self.gamesys
                    .sound_schema()
                    .id_to_samples
                    .get(schema_id)
                    .map(|samples| samples.len())
                    .unwrap_or(0)
            })
            .sum();

        VoiceStats {
            concept_count: voice.tag_maps.len(),
            schema_count: schema_ids.len(),
            sample_count,
            sample_hint: self.voice_sample_hint(voice_idx, voice),
        }
    }

    fn collect_voice_schema_ids(&self, voice: &Voice) -> BTreeSet<i32> {
        let mut ids = BTreeSet::new();
        for tag_db in &voice.tag_maps {
            for schema_id in tag_db.collect_all_data_ids() {
                ids.insert(schema_id);
            }
        }
        ids
    }

    fn voice_sample_hint(&self, voice_idx: usize, voice: &Voice) -> Option<String> {
        let speech_db = self.gamesys.speech_db();

        for (concept_idx, tag_db) in voice.tag_maps.iter().enumerate() {
            let concept_name = speech_db
                .concept_map
                .get_name(concept_idx as u32)
                .cloned()
                .unwrap_or_else(|| format!("#{}", concept_idx));

            let schema_ids: BTreeSet<i32> = tag_db.collect_all_data_ids().into_iter().collect();
            for schema_id in schema_ids {
                if let Some(samples) = self.gamesys.sound_schema().id_to_samples.get(&schema_id) {
                    if let Some(sample) = samples.first() {
                        let preview = sample.sample_name.replace('\\', "/");
                        return Some(format!("{} -> {}", concept_name, preview));
                    }
                }
            }
        }

        info!("No sample hint available for voice {}", voice_idx);
        None
    }

    fn voice_label(&self, idx: usize) -> String {
        format!("{}", idx)
    }

    fn voice_label_with_hint(&self, idx: usize, stats: &VoiceStats) -> String {
        match &stats.sample_hint {
            Some(hint) => format!("{} ({})", self.voice_label(idx), hint),
            None => self.voice_label(idx),
        }
    }
}

#[derive(Clone, Default)]
struct TagSummary {
    occurrences: usize,
    enum_values: BTreeSet<String>,
    min_value: Option<i32>,
    max_value: Option<i32>,
}

impl TagSummary {
    fn add_numeric_range(&mut self, min: i32, max: i32) {
        self.min_value = Some(match self.min_value {
            Some(current) => current.min(min),
            None => min,
        });

        self.max_value = Some(match self.max_value {
            Some(current) => current.max(max),
            None => max,
        });
    }

    fn describe(&self) -> String {
        if !self.enum_values.is_empty() {
            let values: Vec<&String> = self.enum_values.iter().collect();
            let display_count = values.len().min(MAX_ENUM_VALUES_PREVIEW);
            let mut parts: Vec<String> = values[..display_count]
                .iter()
                .map(|v| v.to_string())
                .collect();
            if values.len() > display_count {
                parts.push(format!("... (+{} more)", values.len() - display_count));
            }
            return format!("values [{}]", parts.join(", "));
        }

        if let (Some(min), Some(max)) = (self.min_value, self.max_value) {
            if min == max {
                return format!("value {}", min);
            }
            return format!("range {}..{}", min, max);
        }

        "flag".to_string()
    }
}

struct ParsedSpeechQuery {
    concept: Option<usize>,
    items: Vec<TagQueryItem>,
}

struct VoiceStats {
    concept_count: usize,
    schema_count: usize,
    sample_count: usize,
    sample_hint: Option<String>,
}
