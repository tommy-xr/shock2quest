use anyhow::Result;
use dark::motion::{MotionDB, MotionQuery, MotionQueryItem, MotionQuerySelectionStrategy};
use shock2vr::paths;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use tracing::info;

pub struct MotionAnalyzer {
    motion_db: MotionDB,
    creature_name_to_id: HashMap<String, u32>,
}

impl MotionAnalyzer {
    pub fn new() -> Result<Self> {
        // Load motion database
        let motiondb_path = find_motiondb_file()?;
        info!("Loading motion database from: {}", motiondb_path.display());

        let motiondb_file = File::open(motiondb_path)?;
        let mut motiondb_reader = BufReader::new(motiondb_file);
        let motion_db = MotionDB::read(&mut motiondb_reader);

        // Create creature name mapping based on ActorType enum
        let mut creature_name_to_id = HashMap::new();
        creature_name_to_id.insert("human".to_string(), 0); // ActorType::Human
        creature_name_to_id.insert("playerlimb".to_string(), 1); // ActorType::PlayerLimb
        creature_name_to_id.insert("droid".to_string(), 2); // ActorType::Droid
        creature_name_to_id.insert("overlord".to_string(), 3); // ActorType::Overlord
        creature_name_to_id.insert("arachnid".to_string(), 4); // ActorType::Arachnid

        Ok(Self {
            motion_db,
            creature_name_to_id,
        })
    }

    pub fn list_all_tags_and_animations(
        &self,
        creature_type: u32,
        limit: Option<usize>,
    ) -> Result<()> {
        info!(
            "Querying motion database for creature type {}",
            creature_type
        );

        println!("=== Motion Database Info ===");
        println!("Creature Type: {}", creature_type);

        // Check if creature type is valid
        if creature_type >= self.motion_db.get_creature_type_count() as u32 {
            println!("Invalid creature type: {}.", creature_type);
            println!(
                "Available creature types: 0-{}",
                self.motion_db.get_creature_type_count() - 1
            );
            self.list_available_creature_types();
            return Ok(());
        }

        println!();

        // List all available tags
        let all_tags = self.motion_db.get_all_tag_names();
        let display_tags = if let Some(limit_count) = limit {
            &all_tags[..all_tags.len().min(limit_count)]
        } else {
            &all_tags
        };

        println!("Available tags ({} total):", all_tags.len());
        for (i, tag) in display_tags.iter().enumerate() {
            println!("  {}. +{}", i + 1, tag);
        }

        if let Some(limit_count) = limit {
            if all_tags.len() > limit_count {
                println!("  ... and {} more tags", all_tags.len() - limit_count);
            }
        }

        println!();
        println!("Usage examples:");
        println!("  dark_query motion {} +playspecmotion", creature_type);
        println!("  dark_query motion {} +human +locomote", creature_type);
        println!("  dark_query motion {} +cs:184", creature_type);

        Ok(())
    }

    fn list_available_creature_types(&self) {
        let count = self.motion_db.get_creature_type_count();
        println!("Available creature types (ActorType enum):");

        if count > 0 {
            println!("  0 - Human (try: dark_query motion 0 +human +playspecmotion)");
        }
        if count > 1 {
            println!("  1 - PlayerLimb (try: dark_query motion 1 +playerlimb)");
        }
        if count > 2 {
            println!("  2 - Droid (try: dark_query motion 2 +droid)");
        }
        if count > 3 {
            println!("  3 - Overlord (try: dark_query motion 3 +overlord)");
        }
        if count > 4 {
            println!("  4 - Arachnid (try: dark_query motion 4 +arachnid)");
        }
    }

    pub fn query_with_tags(
        &self,
        creature_type: u32,
        tags: &[String],
        limit: Option<usize>,
    ) -> Result<()> {
        info!(
            "Querying motion database for creature type {} with tags: {:?}",
            creature_type, tags
        );

        // Parse tags into motion query items
        let motion_query_items = parse_tags(tags)?;

        let query = MotionQuery::new(creature_type, motion_query_items)
            .with_selection_strategy(MotionQuerySelectionStrategy::Random);

        let matching_animations = self.motion_db.query_all(query);

        if matching_animations.is_empty() {
            println!(
                "No animations found for creature type {} with tags: {:?}",
                creature_type, tags
            );
            return Ok(());
        }

        let display_animations = if let Some(limit_count) = limit {
            &matching_animations[..matching_animations.len().min(limit_count)]
        } else {
            &matching_animations
        };

        println!("=== Motion Database Query Results ===");
        println!("Creature Type: {}", creature_type);
        println!("Tags: {:?}", tags);
        println!("Matching Animations: {}", matching_animations.len());
        if limit.is_some() {
            println!("Showing first {} animations:", display_animations.len());
        }
        println!();

        println!("Results:");
        for (i, animation) in display_animations.iter().enumerate() {
            println!("  {}. {}", i + 1, animation);
        }

        if let Some(limit_count) = limit {
            if matching_animations.len() > limit_count {
                println!(
                    "\n... and {} more animations",
                    matching_animations.len() - limit_count
                );
            }
        }

        Ok(())
    }

    pub fn parse_creature_type(&self, creature_type_str: &str) -> Result<u32> {
        // Try parsing as number first
        if let Ok(id) = creature_type_str.parse::<u32>() {
            return Ok(id);
        }

        // Try looking up by name
        if let Some(&id) = self
            .creature_name_to_id
            .get(&creature_type_str.to_lowercase())
        {
            return Ok(id);
        }

        anyhow::bail!(
            "Unknown creature type: {}. Use a number (0, 1, 2...) or name (human, midwife, grunt, ninja)",
            creature_type_str
        );
    }
}

fn parse_tags(tags: &[String]) -> Result<Vec<MotionQueryItem>> {
    let mut motion_query_items = Vec::new();

    for tag in tags {
        if !tag.starts_with('+') {
            anyhow::bail!("Tags must start with '+'. Invalid tag: {}", tag);
        }

        let tag_content = &tag[1..]; // Remove the '+'

        // Check if it's a tag with value (e.g., "cs:184")
        if let Some(colon_pos) = tag_content.find(':') {
            let tag_name = &tag_content[..colon_pos];
            let value_str = &tag_content[colon_pos + 1..];

            if let Ok(value) = value_str.parse::<i32>() {
                motion_query_items.push(MotionQueryItem::with_value(tag_name, value));
            } else {
                anyhow::bail!("Invalid tag value: {}. Expected integer after ':'", tag);
            }
        } else {
            // Simple tag without value
            motion_query_items.push(MotionQueryItem::new(tag_content));
        }
    }

    Ok(motion_query_items)
}

fn find_motiondb_file() -> Result<std::path::PathBuf> {
    // Try common locations for motiondb.bin
    let data_motiondb_path = paths::data_root().join("motiondb.bin");
    let possible_paths = ["motiondb.bin", &data_motiondb_path.to_string_lossy()];

    for path in &possible_paths {
        if Path::new(path).exists() {
            return Ok(Path::new(path).to_path_buf());
        }
    }

    anyhow::bail!(
        "Could not find motiondb.bin. Please run from project root or ensure motiondb.bin is in the current directory or Data/ subdirectory."
    );
}
