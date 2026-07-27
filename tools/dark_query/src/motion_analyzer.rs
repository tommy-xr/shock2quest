use anyhow::Result;
use dark::motion::{MotionDB, MotionQuery, MotionQueryItem, MotionQuerySelectionStrategy};
use std::collections::HashMap;
use tracing::info;

use crate::data_loader::open_data_file;

pub struct MotionAnalyzer {
    motion_db: MotionDB,
    creature_name_to_id: HashMap<String, u32>,
}

impl MotionAnalyzer {
    pub fn new() -> Result<Self> {
        // Load motion database
        let motiondb_reader = open_data_file("motiondb.bin")?;
        let motion_db = MotionDB::read(&mut *motiondb_reader.borrow_mut());

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

    /// Print per-clip metadata: motion-stuff (flags, blend length, end
    /// rotation, root translation, duration) and the per-frame motion flags.
    pub fn print_motion_info(&self, name: &str) -> Result<()> {
        if !self.motion_db.has_motion(name) {
            anyhow::bail!("No motion named '{}' in the motion database", name);
        }

        let stuff = self.motion_db.get_motion_stuff(name.to_string());
        let mps = self.motion_db.get_mps_motions(name.to_string());

        println!("=== {} ===", name);
        println!("  flags:        {:#x}", stuff.flags);
        println!("  blend_length: {} ms", stuff.blend_length);
        println!("  end_dir:      {:?}", stuff.end_direction);
        println!(
            "  translation:  [{:.3}, {:.3}, {:.3}]",
            stuff.translation.x, stuff.translation.y, stuff.translation.z
        );
        println!("  duration:     {:.3}", stuff.duration);
        println!(
            "  frames:       {} @ {} fps",
            mps.frame_count, mps.frame_rate
        );
        for frame_flags in &mps.motion_flags {
            println!(
                "  frame {:>4}:   {:?}",
                frame_flags.frame, frame_flags.flags
            );
        }

        // The per-frame root-y stream lives in the clip file (res/motions/
        // <name>_.mc), not the motion database - print its curve when the
        // unpacked file is available
        if let Ok(reader) = open_data_file(&format!("res/motions/{}_.mc", name)) {
            let clip = dark::motion::MotionClip::read(&mut *reader.borrow_mut(), mps);
            let ys: Vec<f32> = clip.root_transforms.iter().map(|m| m.w.y).collect();
            if !ys.is_empty() {
                let n = ys.len();
                println!(
                    "  root y:       start {:.3}  1/4 {:.3}  1/2 {:.3}  3/4 {:.3}  end {:.3}",
                    ys[0],
                    ys[n / 4],
                    ys[n / 2],
                    ys[3 * n / 4],
                    ys[n - 1]
                );
                let (min, max) = ys
                    .iter()
                    .fold((f32::MAX, f32::MIN), |(a, b), y| (a.min(*y), b.max(*y)));
                println!("                min {:.3}  max {:.3}", min, max);
            }
            // Horizontal root motion: net displacement, and how long the
            // root is still (per-frame delta < 1cm) at the clip's tail
            let ps = &clip.root_positions;
            if ps.len() > 1 {
                let net = ps[ps.len() - 1] - ps[0];
                let mut still_frames = 0;
                for i in (1..ps.len()).rev() {
                    let d = ps[i] - ps[i - 1];
                    if (d.x * d.x + d.z * d.z).sqrt() < 0.01 {
                        still_frames += 1;
                    } else {
                        break;
                    }
                }
                let still_secs = still_frames as f32 / mps.frame_rate as f32;
                println!(
                    "  root xz:      net [{:.3}, {:.3}]  still tail {:.2}s ({} frames)",
                    net.x, net.z, still_secs, still_frames
                );
            }
        } else {
            println!(
                "  root y:       (no clip res/motions/{}_.mc in the data)",
                name
            );
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

        // A trailing '?' marks the tag optional (e.g. "+meleecombat?"),
        // matching the optional query items AI scripts use in-game
        let (tag_content, optional) = match tag_content.strip_suffix('?') {
            Some(stripped) => (stripped, true),
            None => (tag_content, false),
        };
        if tag_content.is_empty() {
            anyhow::bail!("Empty tag name in: {}", tag);
        }

        // Check if it's a tag with value (e.g., "cs:184")
        let item = if let Some(colon_pos) = tag_content.find(':') {
            let tag_name = &tag_content[..colon_pos];
            let value_str = &tag_content[colon_pos + 1..];

            if let Ok(value) = value_str.parse::<i32>() {
                MotionQueryItem::with_value(tag_name, value)
            } else {
                anyhow::bail!("Invalid tag value: {}. Expected integer after ':'", tag);
            }
        } else {
            // Simple tag without value
            MotionQueryItem::new(tag_content)
        };
        motion_query_items.push(if optional { item.optional() } else { item });
    }

    Ok(motion_query_items)
}
