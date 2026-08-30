//! Gamesys archetypes: every template that resolves (inheritance-aware) to a
//! model, arranged in its MetaProp hierarchy. Creature templates additionally
//! carry an actor type and the motion-database clip list it can play.

use std::collections::{BTreeSet, HashMap, HashSet};

use dark_viewer::clip_base_name;

use dark::motion::MotionDB;
use dark::properties::{PropCreature, PropModelName, PropSymName, PropTemplateId};
use dark::ss2_entity_info;
use shipyard::{Get, IntoIter, IntoWithId, View, World};
use shock2vr::creature::ActorType;

use crate::ui::quiet_catch;

/// `PropCreature` values, indexed the way `get_creature_definition` is.
const CREATURE_TYPE_NAMES: &[&str] = &[
    "Human",
    "PlayerLimb",
    "Avatar",
    "Rumbler",
    "Droid",
    "Overlord",
    "Arachnid",
    "Monkey",
    "BabyArachnid",
    "Shodan",
];

#[derive(Clone)]
pub struct Archetype {
    pub template_id: i32,
    pub name: String,
    /// Model base name from `PropModelName` (lowercased, no extension).
    pub model_name: String,
    /// `PropCreature`, or None for a non-creature template (weapon, prop, ...).
    pub creature_type: Option<u32>,
    /// The motion database's creature category, from the creature definition.
    pub actor_type: Option<ActorType>,
}

impl Archetype {
    pub fn is_creature(&self) -> bool {
        self.creature_type.is_some()
    }

    pub fn creature_type_name(&self) -> String {
        let Some(creature_type) = self.creature_type else {
            return "-".to_string();
        };
        CREATURE_TYPE_NAMES
            .get(creature_type as usize)
            .map(|n| format!("{n} ({creature_type})"))
            .unwrap_or_else(|| format!("unknown ({creature_type})"))
    }

    pub fn actor_type_name(&self) -> String {
        match &self.actor_type {
            Some(actor) => format!("{actor:?} ({})", actor.clone() as u32),
            None => "unknown".to_string(),
        }
    }

    pub fn model_key(&self) -> String {
        format!("{}.bin", self.model_name)
    }
}

/// A `.mc` motion clip resolved against the motion database.
pub struct ClipInfo {
    /// Clip name as the motion DB knows it (the asset key without `_.mc`).
    pub name: String,
    /// The actor whose tag database references this clip, if any.
    pub actor_type: Option<ActorType>,
    /// The skeleton actually used: `actor_type`, or Human when nothing claims it.
    pub resolved_actor: ActorType,
    /// Mesh supplying that skeleton's joint topology.
    pub model_key: String,
    pub duration: f32,
    pub flags: u32,
    pub frame_count: f32,
    pub frame_rate: i32,
}

impl ClipInfo {
    pub fn actor_label(&self) -> String {
        match &self.actor_type {
            Some(actor) => format!("{actor:?} ({})", actor.clone() as u32),
            None => format!(
                "{:?} (assumed - no actor claims this clip)",
                self.resolved_actor
            ),
        }
    }
}

pub struct ArchetypeDb {
    pub archetypes: HashMap<i32, Archetype>,
    /// Display names for every tree node (grouping ancestors included).
    names: HashMap<i32, String>,
    /// Tree over archetypes and their ancestors, children name-sorted.
    pub children: HashMap<i32, Vec<i32>>,
    pub roots: Vec<i32>,
    parent: HashMap<i32, i32>,
    /// The motion database, or why it failed to load.
    motion_db: Result<MotionDB, String>,
}

impl ArchetypeDb {
    /// Parse the gamesys and motion database. Everything runs under
    /// catch_unwind: a malformed install yields an error string, not a crash.
    pub fn load() -> Result<ArchetypeDb, String> {
        quiet_catch(load_impl).and_then(|r| r)
    }

    pub fn name_of(&self, id: i32) -> String {
        self.names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("Template {id}"))
    }

    /// Tree ancestors of `id`, for opening the path to a selection.
    pub fn ancestors_of(&self, id: i32) -> HashSet<i32> {
        let mut out = HashSet::new();
        let mut current = id;
        while let Some(parent) = self.parent.get(&current) {
            if !out.insert(*parent) {
                break;
            }
            current = *parent;
        }
        out
    }

    /// Clip names playable by this archetype's actor type.
    pub fn clips_for(&self, archetype: &Archetype) -> Result<Vec<String>, String> {
        let motion_db = self.motion_db.as_ref().map_err(|e| e.clone())?;
        if !archetype.is_creature() {
            return Err(format!("'{}' is not a creature", archetype.name));
        }
        let actor = archetype.actor_type.clone().ok_or_else(|| {
            format!(
                "no creature definition for {}",
                archetype.creature_type_name()
            )
        })?;
        Ok(motion_db.get_all_motions_for_creature(actor as u32))
    }

    /// Resolve a `.mc` asset key against the motion database: which actor plays
    /// it, whose skeleton can render it, and the clip's motion-DB metadata.
    pub fn clip_info(&self, asset_key: &str) -> Result<ClipInfo, String> {
        let motion_db = self.motion_db.as_ref().map_err(|e| e.clone())?;
        let name = clip_base_name(asset_key);
        if !motion_db.has_motion(&name) {
            return Err(format!("'{name}' is not in motiondb.bin"));
        }
        // A clip does not name its actor, so ask each actor's tag database
        // whether it can reach this clip; Human is the fallback. Actors come
        // from the archetype table (deduped, ordered - Human wins ties).
        let actor_type = self
            .archetypes
            .values()
            .filter_map(|a| a.actor_type.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .find(|actor| {
                motion_db
                    .get_all_motions_for_creature(actor.clone() as u32)
                    .iter()
                    .any(|m| m.eq_ignore_ascii_case(&name))
            });
        let resolved = actor_type.clone().unwrap_or(ActorType::Human);
        let model_key = self
            .model_for_actor(&resolved)
            .ok_or_else(|| format!("no creature archetype uses the {resolved:?} skeleton"))?;
        let stuff = motion_db.get_motion_stuff(name.clone());
        let mps = motion_db.get_mps_motions(name.clone());
        Ok(ClipInfo {
            name,
            actor_type,
            resolved_actor: resolved,
            model_key,
            duration: stuff.duration,
            flags: stuff.flags,
            frame_count: mps.frame_count,
            frame_rate: mps.frame_rate,
        })
    }

    /// A mesh whose skeleton matches `actor`: the name-first creature archetype
    /// using it (any of them has the right joint topology).
    fn model_for_actor(&self, actor: &ActorType) -> Option<String> {
        self.archetypes
            .values()
            .filter(|a| a.actor_type.as_ref() == Some(actor))
            .min_by_key(|a| a.model_name.clone())
            .map(|a| a.model_key())
    }

    /// Resolve a CLI `--archetype` value: a template id (either sign) or a
    /// case-insensitive name (exact, else unique substring).
    pub fn resolve(&self, wanted: &str) -> Result<i32, String> {
        if let Ok(id) = wanted.parse::<i32>() {
            let id = -id.abs();
            return if self.archetypes.contains_key(&id) {
                Ok(id)
            } else {
                Err(format!("no archetype with template id {id}"))
            };
        }
        let needle = wanted.to_ascii_lowercase();
        let mut matches: Vec<i32> = self
            .archetypes
            .values()
            .filter(|a| a.name.to_ascii_lowercase() == needle)
            .map(|a| a.template_id)
            .collect();
        if matches.is_empty() {
            matches = self
                .archetypes
                .values()
                .filter(|a| a.name.to_ascii_lowercase().contains(&needle))
                .map(|a| a.template_id)
                .collect();
        }
        match matches.as_slice() {
            [id] => Ok(*id),
            [] => Err(format!("no archetype matching '{wanted}'")),
            // Substring hits can run to dozens now that every modeled template
            // is here; list a sample rather than a wall of names.
            many => Err(format!(
                "'{wanted}' is ambiguous ({} matches): {}{}",
                many.len(),
                many.iter()
                    .take(10)
                    .map(|id| self.name_of(*id))
                    .collect::<Vec<_>>()
                    .join(", "),
                if many.len() > 10 { ", ..." } else { "" }
            )),
        }
    }
}

fn load_impl() -> Result<ArchetypeDb, String> {
    let (properties, links, links_with_data) = dark::properties::get();
    let mut reader = shock2vr::data_files::open_data_file("shock2.gam").ok_or_else(|| {
        format!(
            "shock2.gam not found in the game data at {}",
            shock2vr::paths::data_root().display()
        )
    })?;
    let gamesys = dark::gamesys::read(&mut reader, &links, &links_with_data, &properties);
    let entity_info = gamesys.into_entity_info();

    // Materialize every template into a world so property reads below are
    // inheritance-aware (ancestors' components are applied first).
    let mut world = World::new();
    entity_info.initialize_world_with_entities(&mut world, HashMap::new(), |id| id < 0);

    let mut names: HashMap<i32, String> = HashMap::new();
    let mut archetypes: HashMap<i32, Archetype> = HashMap::new();
    {
        let (v_template, v_name, v_model, v_creature) = world
            .borrow::<(
                View<PropTemplateId>,
                View<PropSymName>,
                View<PropModelName>,
                View<PropCreature>,
            )>()
            .map_err(|e| e.to_string())?;
        for (entity, template) in v_template.iter().with_id() {
            let template_id = template.template_id;
            if let Ok(name) = v_name.get(entity) {
                names.insert(template_id, name.0.clone());
            }
            let Ok(model) = v_model.get(entity) else {
                continue;
            };
            let creature_type = v_creature.get(entity).ok().map(|c| c.0);
            let actor_type = creature_type
                .and_then(shock2vr::creature::get_creature_definition)
                .map(|def| def.actor_type.clone());
            archetypes.insert(
                template_id,
                Archetype {
                    template_id,
                    name: names
                        .get(&template_id)
                        .cloned()
                        .unwrap_or_else(|| format!("Template {template_id}")),
                    model_name: model.0.to_ascii_lowercase(),
                    creature_type,
                    actor_type,
                },
            );
        }
    }

    // Tree: archetypes plus every ancestor, parented by the first direct
    // MetaProp parent (multi-parent templates show under one branch).
    let hierarchy = ss2_entity_info::get_hierarchy(&entity_info);
    let mut include: HashSet<i32> = archetypes.keys().copied().collect();
    for id in archetypes.keys() {
        include.extend(ss2_entity_info::get_ancestors(hierarchy, id));
    }
    let mut children: HashMap<i32, Vec<i32>> = HashMap::new();
    let mut parent_map: HashMap<i32, i32> = HashMap::new();
    let mut roots: Vec<i32> = Vec::new();
    for id in &include {
        match hierarchy.get(id).and_then(|parents| parents.first()) {
            Some(parent) if include.contains(parent) => {
                parent_map.insert(*id, *parent);
                children.entry(*parent).or_default().push(*id)
            }
            _ => roots.push(*id),
        }
    }
    let name_of = |id: &i32| {
        names
            .get(id)
            .cloned()
            .unwrap_or_else(|| format!("Template {id}"))
            .to_ascii_lowercase()
    };
    for siblings in children.values_mut() {
        siblings.sort_by_key(name_of);
    }
    roots.sort_by_key(name_of);

    let motion_db = quiet_catch(|| {
        let mut reader = shock2vr::data_files::open_data_file("motiondb.bin")
            .ok_or_else(|| "motiondb.bin not found in the game data".to_string())?;
        Ok(MotionDB::read(&mut reader))
    })
    .and_then(|r| r);

    Ok(ArchetypeDb {
        archetypes,
        names,
        children,
        roots,
        parent: parent_map,
        motion_db,
    })
}
