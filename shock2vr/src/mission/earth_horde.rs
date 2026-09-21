//! Opt-in Earth containment experiment. Real mission geometry and ordinary
//! inventory/combat/MFDs; only the scenario setup and wave schedule are new.
//! The director is an ordinary saved script, so a loaded run never re-seeds
//! the arena, reissues starter gear, or replays a completed wave's reward.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use cgmath::{Deg, InnerSpace, Matrix4, Point3, Quaternion, Rotation3, SquareMatrix, vec3};
use dark::{properties::*, ss2_entity_info::SystemShock2EntityInfo};
use engine::assets::asset_cache::AssetCache;
use rand::{Rng, RngCore, SeedableRng};
use serde::{Deserialize, Serialize};
use shipyard::{
    EntitiesView, EntityId, Get, IntoIter, IntoWithId, UniqueView, UniqueViewMut, View, World,
};

use super::{
    MissionCore, PlayerInfo, PlayerLifeState,
    entity_creator::{self, CreateEntityOptions},
    entity_populator::{EntityPopulation, EntityPopulator},
};
use crate::{
    game_scene::DebuggableScene,
    physics::{InternalCollisionGroups, PhysicsWorld},
    quest_info::QuestInfo,
    scripts::{
        Effect, MessagePayload, Script, ScriptRestoreContext, ScriptState, ScriptStateError,
    },
    time::Time,
};

const DIRECTOR: i32 = 60_000;
const MARKER_START: i32 = 60_100;
const ECOLOGY: i32 = 60_000;
const STATE_KEY: &str = "shock2vr.earth_horde";
const SEED: u64 = 0x4541525448;
const FINAL_WAVE: u32 = 10;
const MAX_ALIVE: usize = 15;
const OS_STATIONS: i32 = 60_300;
const OS_WAVES: [u32; 4] = [0, 3, 6, 9];

// Unlocks are monotonic: a debug jump backwards never revokes an earned station.
fn unlock_os_stations(world: &World, wave: u32) -> Vec<Effect> {
    let (ids, locked) = world
        .borrow::<(View<PropTemplateId>, View<PropLocked>)>()
        .unwrap();
    let mut effects = vec![];
    for (entity_id, (id, locked)) in (&ids, &locked).iter().with_id() {
        let index = id.template_id - OS_STATIONS;
        if let Some(required) = usize::try_from(index).ok().and_then(|i| OS_WAVES.get(i)) {
            if locked.0 && wave >= *required {
                effects.push(Effect::SetLocked {
                    entity_id,
                    locked: false,
                });
                effects.push(Effect::SetRenderAlpha {
                    entity_id,
                    alpha: 1.0,
                });
                effects.push(Effect::ShowMessage {
                    text: format!(
                        "OS upgrade station online: wave {required}. One free upgrade available."
                    ),
                });
            }
        }
    }
    effects
}

const WAVE_COUNTS: [u32; 10] = [6, 9, 12, 15, 18, 22, 26, 30, 34, 38];
// Each unlocked archetype gets a guaranteed slot every wave. Pipe and shotgun
// open every wave, so neither melee nor ranged pressure depends on RNG.
const ENEMY_INTRODUCTIONS: &[(&str, u32)] = &[
    ("OG-Pipe", 1),
    ("OG-Shotgun", 1),
    ("Maintenance", 2),
    ("Protocol Droid", 2),
    ("Midwife", 3),
    ("Blue Monkey", 3),
    ("Arachnid", 4),
    ("Baby Arachnid", 5),
    ("Security", 6),
    ("Red Monkey", 6),
    ("Assassin", 7),
    ("OG-Grenade", 7),
    ("Rumbler", 8),
    ("Assault", 8),
    ("Overlord", 9),
    ("Greater Over.", 10),
    ("SHODAN", 10),
];

// Floor positions surveyed against Earth's actual collision geometry. The
// subway is a separate AI arena: creatures cannot ride the player gravshafts.
pub(super) const SITES: &[[f32; 3]] = &[
    [-8.0, 19.8, 24.0],
    [4.0, 19.8, 24.0],
    [20.0, 19.8, 24.0],
    [28.0, 19.8, 24.0],
    [11.6, 22.4, 44.0],
    [8.0, 22.4, 48.0],
    [11.6, 22.4, 52.0],
    [12.0, 22.4, 56.0],
    [0.0, 0.8, 4.0],
    [21.0, 0.8, 4.0],
    [0.0, 0.8, 10.0],
    [21.0, 0.8, 10.0],
    [-4.0, 20.0, 24.0],
    [25.0, 20.0, 24.0],
    [-6.0, 0.8, 4.0],
    [28.0, 0.8, 4.0],
];

pub(crate) fn asset_mission(name: &str) -> &str {
    if is_horde(name) { "earth.mis" } else { name }
}

fn is_horde(name: &str) -> bool {
    name.eq_ignore_ascii_case("earth_horde")
        || name.eq_ignore_ascii_case("earth_horde_test")
        || name.eq_ignore_ascii_case("earth_horde_final")
}

pub(crate) fn wrap_population(
    name: &str,
    inner: Box<dyn EntityPopulator>,
    fresh: Arc<AtomicBool>,
) -> Box<dyn EntityPopulator> {
    if is_horde(name) {
        Box::new(HordePopulation {
            inner,
            fresh,
            quick: name.eq_ignore_ascii_case("earth_horde_test"),
            final_preview: name.eq_ignore_ascii_case("earth_horde_final"),
        })
    } else {
        inner
    }
}

struct HordePopulation {
    inner: Box<dyn EntityPopulator>,
    fresh: Arc<AtomicBool>,
    quick: bool,
    final_preview: bool,
}

impl EntityPopulator for HordePopulation {
    fn populate(
        &self,
        gamesys: &SystemShock2EntityInfo,
        level: &SystemShock2EntityInfo,
        names: &HashMap<i32, String>,
        world: &mut World,
    ) -> EntityPopulation {
        let mut population = self.inner.populate(gamesys, level, names, world);
        if population.template_to_entity_id.contains_key(&DIRECTOR) {
            return population;
        }
        self.fresh.store(true, Ordering::Relaxed);
        // Disable the authored training graph before any script initializes.
        // Retain room gravity and decorative light behavior. Doors stay closed
        // and locked, with no script/linked switch able to reopen them.
        let originals: Vec<_> = population
            .template_to_entity_id
            .values()
            .map(|id| id.0)
            .collect();
        for entity in originals {
            if world.borrow::<View<PropAI>>().unwrap().get(entity).is_ok() {
                world.delete_entity(entity);
                population
                    .template_to_entity_id
                    .retain(|_, id| id.0 != entity);
                continue;
            }
            let door = world
                .borrow::<View<PropTranslatingDoor>>()
                .unwrap()
                .get(entity)
                .ok()
                .cloned();
            if let Some(mut door) = door {
                door.state = 0;
                door.base_location = door.base_closed_location;
                let mut pos = world
                    .borrow::<View<PropPosition>>()
                    .unwrap()
                    .get(entity)
                    .unwrap()
                    .clone();
                pos.position = door.base_closed_location;
                world.add_component(entity, (door, pos));
            }
            let kept = world
                .borrow::<View<PropScripts>>()
                .unwrap()
                .get(entity)
                .ok()
                .map(|p| {
                    p.scripts
                        .iter()
                        .filter(|s| {
                            matches!(
                                s.to_ascii_lowercase().as_str(),
                                "baselight" | "lightsoundon" | "coreroom" | "zerogravroom"
                            )
                        })
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            world.add_component(
                entity,
                (
                    PropScripts {
                        scripts: kept,
                        inherits: false,
                    },
                    PropLocked(true),
                ),
            );
            // Training supplies remain decorative behind the sealed doors.
            // Removing links also stops indirect sound/teleport/reward chains.
            world.add_component(
                entity,
                Links {
                    to_links: Vec::new(),
                },
            );
        }

        let mut add = |id: i32, template: i32, name: &str, pos: [f32; 3], yaw: f32| {
            let entity = world.add_entity(());
            entity_creator::initialize_entity_with_props(
                template,
                gamesys,
                world,
                entity,
                &HashMap::new(),
            );
            world.add_component(
                entity,
                (
                    PropTemplateId { template_id: id },
                    PropSymName(name.to_owned()),
                    PropObjName(format!("horde_object: \"{name}\"")),
                    PropPosition {
                        position: pos.into(),
                        cell: u16::MAX,
                        rotation: Quaternion::from_angle_y(Deg(yaw)),
                    },
                    Links {
                        to_links: Vec::new(),
                    },
                ),
            );
            population
                .template_to_entity_id
                .insert(id, WrappedEntityId(entity));
            entity
        };
        let director = add(
            DIRECTOR,
            -1594,
            "Containment - Ready / Endless",
            [4.8, 21.5, 32.5],
            0.0,
        );
        // Stage the component changes until after the closure releases world.
        let mut markers = Vec::new();
        for (index, site) in SITES.iter().enumerate() {
            markers.push(add(
                MARKER_START + index as i32,
                -327,
                "Containment spawn",
                [site[0], site[1] + 2.0, site[2]],
                0.0,
            ));
        }
        // Mount the cabinet backs just into the surveyed vertical wall planes
        // (north z33.2, south z8, east x55), clear of the sloped buttresses.
        let supply = add(
            60_010,
            -463,
            "West street supplies",
            [0.0, 23.55, 32.7],
            0.0,
        );
        let specialty = add(
            60_011,
            -463,
            "South street ammunition",
            [25.0, 23.55, 8.5],
            180.0,
        );
        let east_supply = add(
            60_012,
            -463,
            "Far east street supplies",
            [54.5, 23.55, 24.0],
            90.0,
        );
        // Retail replicators are an assembly: RepBase is only the cabinet.
        // Match the separate RepScreen and its authored local offset.
        for (index, position, yaw) in [
            (0, [0.0, 23.40, 33.04], 0.0),
            (1, [25.0, 23.40, 8.16], 180.0),
            (2, [54.84, 23.40, 24.0], 90.0),
        ] {
            add(60_040 + index, -464, "Replicator display", position, yaw);
        }
        for (index, (template, name, position, yaw)) in [
            (-581, "Stats Trainer", [-11.2, 2.8, 4.0], -90.0),
            (-1436, "Tech Trainer", [32.8, 2.8, 4.0], 90.0),
            (-1437, "Weapon Trainer", [19.3, 2.8, 12.4], 0.0),
            (-1583, "Psi Trainer", [24.0, 2.8, 12.4], 0.0),
        ]
        .iter()
        .enumerate()
        {
            add(60_020 + index as i32, *template, name, *position, *yaw);
        }
        // West landing wall is continuous here (x6.8); the old wider bank
        // crossed the doorway at z50. Keep all four clear of its light fixture.
        let mut os_stations = vec![];
        for (index, wave) in OS_WAVES.iter().enumerate() {
            os_stations.push(add(
                OS_STATIONS + index as i32,
                -2307,
                &format!(
                    "OS upgrade — {}",
                    if *wave == 0 {
                        "available now".into()
                    } else {
                        format!("wave {wave}")
                    }
                ),
                [7.25, 24.0, 43.0 + index as f32 * 1.6],
                -90.0,
            ));
        }
        let outlets = [
            (
                supply,
                60_030,
                add(60_030, -327, "Supply outlet", [0.0, 21.0, 30.8], 0.0),
            ),
            (
                specialty,
                60_031,
                add(60_031, -327, "Ammunition outlet", [25.0, 21.0, 10.0], 180.0),
            ),
            (
                east_supply,
                60_032,
                add(
                    60_032,
                    -327,
                    "Far east supply outlet",
                    [52.8, 21.0, 24.0],
                    90.0,
                ),
            ),
        ];
        let containment = super::earth_containment::populate(&mut add);
        drop(add);
        for (index, station) in os_stations.into_iter().enumerate() {
            world.add_component(
                station,
                (
                    PropLocked(index > 0),
                    PropRenderAlpha(if index == 0 { 1.0 } else { 0.35 }),
                ),
            );
        }
        super::earth_containment::configure(world, containment, director);
        for (shop, template, outlet) in outlets {
            world.add_component(
                shop,
                Links {
                    to_links: vec![ToLink {
                        to_template_id: template,
                        to_entity_id: Some(WrappedEntityId(outlet)),
                        link: Link::Replicator,
                    }],
                },
            );
        }
        world.add_component(
            director,
            (
                PropScripts {
                    scripts: vec!["EarthHorde".into()],
                    inherits: false,
                },
                PropEcoType(if self.quick {
                    1
                } else if self.final_preview {
                    2
                } else {
                    0
                }),
            ),
        );
        for marker in markers {
            world.add_component(
                marker,
                PropScripts {
                    scripts: vec![],
                    inherits: false,
                },
            );
        }
        for (entity, items, costs) in [
            (
                supply,
                [
                    "Standard Clip",
                    "Med Patch",
                    "Psi Booster",
                    "Maintenance Tool",
                    "Detox Patch",
                    "Anti-Annelid Toxin",
                ],
                [8, 10, 8, 12, 8, 10],
            ),
            (
                east_supply,
                [
                    "Standard Clip",
                    "Med Patch",
                    "Psi Booster",
                    "Maintenance Tool",
                    "Detox Patch",
                    "Anti-Annelid Toxin",
                ],
                [8, 10, 8, 12, 8, 10],
            ),
            (
                specialty,
                [
                    "Pellet Shot Box",
                    "AP Clip",
                    "Portable Battery",
                    "Rifled Slug Box",
                    "Psi Booster",
                    "Med Patch",
                ],
                [10, 12, 10, 8, 8, 10],
            ),
        ] {
            world.add_component(
                entity,
                (
                    PropReplicatorContents {
                        object_names: items.map(str::to_ascii_lowercase),
                        costs,
                    },
                    PropReplicatorHackedContents {
                        object_names: items.map(str::to_ascii_lowercase),
                        costs: costs.map(|c| c * 3 / 4),
                    },
                ),
            );
        }
        population
    }
}

pub(crate) fn provision(core: &mut MissionCore, assets: &mut AssetCache) {
    for (name, _) in ENEMY_INTRODUCTIONS {
        assert!(
            core.template_name_to_template_id
                .contains_key(&name.to_ascii_lowercase()),
            "unknown horde enemy: {name}"
        );
    }
    for contents in core
        .world
        .borrow::<View<PropReplicatorContents>>()
        .unwrap()
        .iter()
    {
        for name in contents.object_names.iter().filter(|name| !name.is_empty()) {
            assert!(
                core.template_name_to_template_id.contains_key(name),
                "unknown horde shop item: {name}"
            );
        }
    }
    {
        let mut quests = core.world.borrow::<UniqueViewMut<QuestInfo>>().unwrap();
        let stats = quests.player_stats_mut();
        stats.strength = 2;
        stats.endurance = 2;
        stats.psionic_ability = 2;
        stats.skills.standard_weapons = 1;
        stats.psi_tier = 1;
        stats.cyber_modules = 16;
        stats.nanites = 80;
        // This experiment supplies ready-to-use Toxin-A, not Hydro's research
        // quest. Seed completed knowledge before any shop vial initializes;
        // ResearchableScript normalizes every future copy, including on load.
        let research = quests.research_mut();
        research.begin(-1341, 0, 0);
        research.advance(-1341, 0.0, 1, 1.0, 0.0, None, 0);
    }
    crate::difficulty::refresh_player_pools(&core.world, true);
    core.teleport_player(vec3(11.6, 23.36, 42.0))
        .expect("surveyed Earth horde start");
    core.world
        .borrow::<UniqueViewMut<PlayerInfo>>()
        .unwrap()
        .rotation = Quaternion::from_angle_y(Deg(-90.0));
    for template in [-928, -17, -247, -1358, -1358, -52, -57, -57] {
        core.spawn_into_backpack(assets, template)
            .expect("horde starter item fits backpack");
    }
    let mut rng = rand::thread_rng();
    for name in [
        "Shotgun",
        "Laser Pistol",
        "Maintenance Tool",
        "5 Nanites",
        "EXP Cookies",
    ] {
        let site = SITES[rng.gen_range(0..8)];
        core.create_entity_by_template_name(
            assets,
            name,
            Point3::new(site[0], site[1] + 0.5, site[2]),
            Quaternion::from_angle_y(Deg(0.0)),
        );
    }
    // The normal inventory/hand path handles actual weapon selection. Both
    // the pistol and amp are available from the outset, without maxed stats.
}

/// The slain enemy's own entity, when it is still around to hold loot. A
/// creature keeps its entity (and its `creaturecontainer` loot panel) as a
/// corpse; one replaced by its Corpse/Flinderize gibs does not.
fn corpse_container(world: &World, id: u64) -> Option<EntityId> {
    let entity = EntityId::from_inner(id)?;
    if !world
        .borrow::<EntitiesView>()
        .is_ok_and(|entities| entities.is_alive(entity))
    {
        return None;
    }
    // Without `Links` there is nowhere to record the `Contains`.
    world.borrow::<View<Links>>().ok()?.get(entity).ok()?;
    Some(entity)
}

/// Only unlooted contents still owned by expired corpses are removed. Picked
/// up items have left those Contains links, so inventory and held loot survive.
pub(crate) fn wave_jump_message(world: &World, wave: u32) -> Effect {
    let scripts = world.borrow::<View<PropScripts>>().unwrap();
    scripts
        .iter()
        .with_id()
        .find(|(_, s)| {
            s.scripts
                .iter()
                .any(|name| name.eq_ignore_ascii_case("EarthHorde"))
        })
        .map(|(to, _)| Effect::Send {
            msg: crate::scripts::Message {
                to,
                payload: MessagePayload::StartHordeWave {
                    wave: wave.clamp(1, 100),
                },
            },
        })
        .unwrap_or(Effect::ShowMessage {
            text: "Start wave is available in Earth horde.".into(),
        })
}

fn expired_corpses(world: &World) -> Vec<Effect> {
    cleanup_enemies(world, false)
}

fn cleanup_enemies(world: &World, include_living: bool) -> Vec<Effect> {
    let (types, hp, links) = world
        .borrow::<(View<PropEcoType>, View<PropHitPoints>, View<Links>)>()
        .unwrap();
    let mut ids: Vec<_> = (&types, &hp)
        .iter()
        .with_id()
        .filter(|(_, (tag, hp))| {
            (tag.0 > ECOLOGY || super::earth_containment::is_containment_type(tag.0))
                && (hp.hit_points <= 0 || (include_living && tag.0 > ECOLOGY))
        })
        .map(|(id, _)| id)
        .collect();
    let mut seen: std::collections::HashSet<_> = ids.iter().copied().collect();
    let mut index = 0;
    while index < ids.len() {
        if let Ok(contents) = links.get(ids[index]) {
            for link in &contents.to_links {
                if matches!(link.link, Link::Contains(_)) {
                    if let Some(child) = link.to_entity_id.filter(|child| seen.insert(child.0)) {
                        ids.push(child.0);
                    }
                }
            }
        }
        index += 1;
    }
    ids.into_iter()
        .rev()
        .map(|entity_id| Effect::DestroyEntity { entity_id })
        .collect()
}

/// How long a wave's opening card stays up. Shorter than the mission's own
/// title card: it interrupts a fight that is already starting.
const WAVE_CARD_DURATION: Duration = Duration::from_secs(3);

// One song per authored level with a music bed, in deck progression order:
// MedSci 1/2, Engineering 1/2, Hydroponics 1/2, Operations 2/3/4,
// Recreation 1, Command 1/2, Rickenbacker 1 (their SONGPARAMS values).
const WAVE_SONGS: &[&str] = &[
    "song08", "song06", "engsong", "engsong2", "song02", "song09", "song04", "song03", "song10",
    "song07", "song13", "song11", "song14",
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
enum Phase {
    #[default]
    Rest,
    Assault,
    Victory,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Enemy {
    id: u64,
    position: [f32; 3],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct HordeDirector {
    initialized: bool,
    // Audio is scene-local and must be re-established after loading a save.
    #[serde(skip)]
    music_synced: bool,
    quick: bool,
    phase: Phase,
    wave: u32,
    spawned: u32,
    kills: u32,
    clock: f32,
    elapsed: f32,
    next_spawn: f32,
    next_status: f32,
    seed: u64,
    enemies: Vec<Enemy>,
    containment: super::earth_containment::Containment,
    supplies: super::earth_supplies::SupplyDrops,
    tech: super::earth_tech::TechGrowth,
    last_spawn_site: Option<i32>,
}

impl Default for HordeDirector {
    fn default() -> Self {
        Self {
            initialized: false,
            music_synced: false,
            quick: false,
            phase: Phase::Rest,
            wave: 0,
            spawned: 0,
            kills: 0,
            clock: 60.0,
            elapsed: 0.0,
            next_spawn: 0.0,
            next_status: 0.0,
            seed: SEED,
            enemies: vec![],
            containment: Default::default(),
            supplies: Default::default(),
            tech: Default::default(),
            last_spawn_site: None,
        }
    }
}

impl HordeDirector {
    fn final_wave(&self) -> u32 {
        if self.quick { 3 } else { FINAL_WAVE }
    }
    fn rest_seconds(&self) -> f32 {
        if self.quick { 8.0 } else { 60.0 }
    }
    fn quota(&self) -> u32 {
        if self.quick {
            (2 + self.wave).max(self.enemy_roster().len() as u32)
        } else {
            WAVE_COUNTS[self.wave.saturating_sub(1).min(FINAL_WAVE - 1) as usize]
                .saturating_add(self.wave.saturating_sub(FINAL_WAVE).saturating_mul(6))
        }
    }
    fn assault_seconds(&self) -> f32 {
        if self.quick {
            12.0 + self.wave as f32 * 4.0
        } else {
            60.0 + self.wave.saturating_sub(1) as f32 * 8.0
        }
    }
    fn enemy_roster(&self) -> Vec<&'static str> {
        let unlocked = ENEMY_INTRODUCTIONS
            .iter()
            .filter(|(_, debut)| *debut <= self.wave.max(1));
        // New enemies arrive promptly after the opening pair; all returning
        // types still appear before weighted reinforcements begin.
        let mut roster = vec!["OG-Pipe", "OG-Shotgun"];
        roster.extend(
            unlocked
                .clone()
                .filter(|(_, debut)| *debut == self.wave && *debut > 1)
                .map(|(name, _)| *name),
        );
        roster.extend(
            unlocked
                .filter(|(_, debut)| *debut > 1 && *debut != self.wave)
                .map(|(name, _)| *name),
        );
        roster
    }
    fn next_enemy(&mut self) -> &'static str {
        let roster = self.enemy_roster();
        if let Some(name) = roster.get(self.spawned as usize) {
            return name;
        }
        // Reinforcements favor ordinary enemies during the finite run.
        // Endless progressively weights the late introductions more heavily;
        // counts and assault duration keep growing without a gameplay cap.
        let endless = self.wave.saturating_sub(FINAL_WAVE).min(32);
        let weighted: Vec<_> = ENEMY_INTRODUCTIONS
            .iter()
            .filter(|(name, debut)| *debut <= self.wave.max(1) && *name != "SHODAN")
            .map(|(name, debut)| (*name, if *debut >= 6 { 1 + endless as usize } else { 4 }))
            .collect();
        let mut choice = self.roll(weighted.iter().map(|(_, weight)| weight).sum());
        for (name, weight) in weighted {
            if choice < weight {
                return name;
            }
            choice -= weight;
        }
        unreachable!("weighted enemy choice must resolve")
    }
    fn roll(&mut self, count: usize) -> usize {
        let mut rng = rand::rngs::StdRng::seed_from_u64(self.seed);
        self.seed = rng.next_u64();
        rng.gen_range(0..count)
    }
    fn music(&self) -> Effect {
        Effect::SetBackgroundMusic {
            song: (self.phase == Phase::Assault).then(|| {
                WAVE_SONGS[self.wave.saturating_sub(1) as usize % WAVE_SONGS.len()].to_owned()
            }),
        }
    }

    fn jump_to_wave(&mut self, world: &World, wave: u32) -> Effect {
        if !self.initialized
            || !world
                .borrow::<UniqueView<PlayerLifeState>>()
                .is_ok_and(|life| life.is_alive())
        {
            return Effect::NoEffect;
        }
        let mut effects = cleanup_enemies(world, true);
        self.enemies.clear();
        self.kills = 0;
        self.wave = wave.clamp(1, 100) - 1;
        effects.push(self.start_wave(world));
        Effect::Multiple(effects)
    }

    fn start_wave(&mut self, world: &World) -> Effect {
        let mut effects = expired_corpses(world);
        self.wave = self.wave.saturating_add(1);
        effects.extend(self.supplies.start_wave(world, self.wave, self.quick));
        self.phase = Phase::Assault;
        effects.push(self.music());
        effects.extend(unlock_os_stations(world, self.wave));
        self.spawned = 0;
        self.clock = 0.0;
        self.next_spawn = 0.0;
        self.next_status = 4.0;
        // The wave card, on the same centered banner the Earth mission opens
        // with - a wave boundary is the one moment in a run worth interrupting
        // the view for. The periodic `status` line keeps reporting the wave on
        // the message channel from four seconds in.
        effects.push(Effect::ShowBanner {
            text: format!("Wave {}", self.wave),
            duration: WAVE_CARD_DURATION,
        });
        Effect::Multiple(effects)
    }
    fn status(&self) -> Effect {
        let text = match self.phase {
            Phase::Rest => format!(
                "Wave {} in {}s | Shops: street | Trainers: subway | OS: landing",
                self.wave + 1,
                self.clock.ceil() as u32
            ),
            Phase::Assault => format!(
                "Wave {} | {} attackers left | {} arriving",
                self.wave,
                self.enemies.len(),
                self.quota().saturating_sub(self.spawned)
            ),
            Phase::Victory => format!(
                "CONTAINMENT COMPLETE | {} kills | {}m {}s | Use READY for endless",
                self.kills,
                self.elapsed as u32 / 60,
                self.elapsed as u32 % 60
            ),
            Phase::Failed => format!(
                "Containment lost - wave {} | {} kills",
                self.wave, self.kills
            ),
        };
        tracing::info!(target: "earth_horde", wave=self.wave, phase=?self.phase, spawned=self.spawned, kills=self.kills, "{text}");
        Effect::ShowMessage { text }
    }
    fn clear_wave(&mut self) -> Effect {
        self.phase = if self.wave == self.final_wave() {
            Phase::Victory
        } else {
            Phase::Rest
        };
        self.clock = self.rest_seconds();
        self.next_status = 5.0;
        let effects = vec![
            self.music(),
            Effect::AwardNanites {
                amount: 30 + self.wave.min(20) as i32 * 5,
            },
            Effect::AwardXP {
                amount: 8 + self.wave.min(20) as i32 * 2,
            },
            Effect::ShowMessage {
                text: format!(
                    "Wave {} cleared! +{} nanites, +{} cyber modules",
                    self.wave,
                    30 + self.wave.min(20) * 5,
                    8 + self.wave.min(20) * 2
                ),
            },
        ];
        Effect::Multiple(effects)
    }
    /// Roll this kill's drop into the corpse's own loot container, so it is
    /// searched like any authored creature's loot (and expires with the corpse,
    /// see `expired_corpses`) instead of landing on the floor. The roll happens
    /// first either way, so a corpse-less kill does not shift the seeded stream.
    fn bonus_loot(&mut self, world: &World, corpse: u64, position: [f32; 3]) -> Effect {
        let template_id = match self.roll(5) {
            0 => -938,
            1 | 2 => -87,
            _ => return Effect::NoEffect,
        };
        match corpse_container(world, corpse) {
            Some(container_entity_id) => Effect::CreateEntityInContainer {
                template_id,
                container_entity_id,
            },
            // Nothing survived the kill to loot (a droid replaced by its
            // Corpse/Flinderize gibs): drop it where the enemy fell rather
            // than losing the reward.
            None => Effect::CreateEntity {
                template_id,
                position: Point3::from(position),
                orientation: Quaternion::from_angle_y(Deg(0.0)),
                root_transform: Matrix4::identity(),
                options: CreateEntityOptions::default(),
            },
        }
    }
    fn spawn(&mut self, world: &World, physics: &PhysicsWorld) -> Option<Effect> {
        let player = world.borrow::<UniqueView<PlayerInfo>>().ok()?.pos;
        let positions = world.borrow::<View<PropPosition>>().ok()?;
        let templates = world.borrow::<View<PropTemplateId>>().ok()?;
        let mut candidates: Vec<_> = (&templates, &positions)
            .iter()
            .with_id()
            .filter(|(_, (id, pos))| {
                (MARKER_START..MARKER_START + SITES.len() as i32).contains(&id.template_id)
                    && (pos.position.y < 10.0) == (player.y < 10.0)
                    && (pos.position - player).magnitude2() > 8.0 * 8.0
            })
            .map(|(entity, (template, position))| (template.template_id, entity, position.position))
            .collect();
        // HashMap/ECS iteration order must not change a seeded run.
        candidates.sort_by_key(|(id, _, _)| *id);
        let hidden: Vec<_> = candidates
            .iter()
            .copied()
            .filter(|(_, id, pos)| {
                let delta = player - *pos;
                physics
                    .ray_cast2(
                        Point3::new(pos.x, pos.y, pos.z),
                        delta.normalize(),
                        delta.magnitude(),
                        InternalCollisionGroups::WORLD,
                        Some(*id),
                        true,
                    )
                    .is_some()
            })
            .collect();
        // Open street sightlines may cover all markers. The existing SpawnSFX
        // telegraphs a distant fallback; never spawn at the player's feet.
        let mut sites = if hidden.is_empty() {
            candidates
        } else {
            hidden
        };
        if sites.is_empty() {
            return None;
        }
        if sites.len() > 1 {
            sites.retain(|(id, _, _)| Some(*id) != self.last_spawn_site);
        }
        let (site, marker, _) = sites[self.roll(sites.len())];
        self.last_spawn_site = Some(site);
        let template_name = self.next_enemy().to_owned();
        Some(Effect::SpawnEcologyEntity {
            template_name,
            spawn_point: marker,
            ecology_type: Some(ECOLOGY + self.wave as i32),
            goto_player: true,
        })
    }
}

impl Script for HordeDirector {
    fn update(
        &mut self,
        entity: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        let dt = time.elapsed.as_secs_f32();
        if dt <= 0.0 {
            return Effect::NoEffect;
        }
        let mut effects = Vec::new();
        if !self.initialized {
            self.initialized = true;
            self.seed = rand::random();
            let mode = world
                .borrow::<View<PropEcoType>>()
                .unwrap()
                .get(entity)
                .map(|p| p.0)
                .unwrap_or(0);
            self.quick = mode == 1;
            self.wave = if mode == 2 {
                FINAL_WAVE
            } else {
                crate::dev_params::get(crate::dev_params::HORDE_START_WAVE) as u32
            }
            .clamp(1, 100)
                - 1;
            self.clock = self.rest_seconds();
            self.next_status = 5.0;
            effects.extend(unlock_os_stations(world, self.wave + 1));
            effects.push(Effect::ShowMessage { text: "EARTH: CONTAINMENT | Pistol + psi amp in inventory | Trainers in subway; shops on street; OS bank at the stair landing: start / waves 3/6/9".into() });
        }
        if !self.music_synced {
            effects.push(self.music());
            self.music_synced = true;
        }
        if self.phase != Phase::Failed
            && world
                .borrow::<UniqueView<PlayerLifeState>>()
                .is_ok_and(|life| !life.is_alive())
        {
            self.phase = Phase::Failed;
            effects.push(self.music());
            effects.push(self.status());
        }
        if self.phase == Phase::Failed {
            return Effect::Multiple(effects);
        }
        if self.phase != Phase::Victory {
            self.elapsed += dt;
        }
        self.next_status -= dt;
        if self.next_status <= 0.0 {
            effects.push(self.status());
            self.next_status = 5.0;
        }
        effects.extend(self.supplies.update(world, dt, self.phase == Phase::Rest));
        effects.extend(self.tech.update(
            world,
            physics,
            dt,
            self.phase == Phase::Assault,
            self.wave,
            self.quick,
        ));
        effects.extend(self.containment.update(
            world,
            dt,
            self.phase == Phase::Assault,
            self.quick,
            self.wave,
        ));
        match self.phase {
            Phase::Rest => {
                self.clock -= dt;
                if self.clock <= 0.0 {
                    effects.push(self.start_wave(world));
                }
            }
            Phase::Assault => {
                self.clock += dt;
                let live: Vec<Enemy> = {
                    let (types, hp, positions) = world
                        .borrow::<(View<PropEcoType>, View<PropHitPoints>, View<PropPosition>)>()
                        .unwrap();
                    (&types, &hp, &positions)
                        .iter()
                        .with_id()
                        .filter(|(_, (t, hp, _))| {
                            t.0 == ECOLOGY + self.wave as i32 && hp.hit_points > 0
                        })
                        .map(|(id, (_, _, pos))| Enemy {
                            id: id.inner(),
                            position: pos.position.into(),
                        })
                        .collect()
                };
                for born in &live {
                    if !self.enemies.iter().any(|old| old.id == born.id) {
                        self.spawned += 1;
                    }
                }
                let defeated: Vec<_> = self
                    .enemies
                    .iter()
                    .filter(|old| !live.iter().any(|now| now.id == old.id))
                    .map(|old| (old.id, old.position))
                    .collect();
                for (corpse, position) in defeated {
                    self.kills += 1;
                    effects.push(self.bonus_loot(world, corpse, position));
                }
                self.enemies = live;
                // Creation applies after this update. Count only observed
                // children, so a failed creation never consumes wave supply.
                self.next_spawn -= dt;
                if self.spawned >= self.quota()
                    && self.enemies.is_empty()
                    && self.clock >= self.assault_seconds()
                {
                    effects.push(self.clear_wave());
                } else if self.spawned < self.quota()
                    && self.enemies.len() < MAX_ALIVE
                    && self.next_spawn <= 0.0
                {
                    if let Some(spawn) = self.spawn(world, physics) {
                        effects.push(spawn);
                        self.next_spawn = self.assault_seconds() / self.quota() as f32;
                    } else {
                        self.next_spawn = 1.0;
                    }
                }
            }
            Phase::Victory | Phase::Failed => {}
        }
        Effect::Multiple(effects)
    }
    fn handle_message(
        &mut self,
        _entity: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        if let MessagePayload::StartHordeWave { wave } = msg {
            return self.jump_to_wave(world, *wave);
        }
        if let MessagePayload::TurnOn { from } = msg {
            if self.phase != Phase::Failed {
                return self.containment.replenish(world, *from);
            }
        }
        if matches!(msg, MessagePayload::Frob)
            && self.initialized
            && world
                .borrow::<UniqueView<PlayerLifeState>>()
                .is_ok_and(|life| life.is_alive())
        {
            if matches!(self.phase, Phase::Rest | Phase::Victory) {
                return self.start_wave(world);
            }
            return self.status();
        }
        Effect::NoEffect
    }
    fn script_state_key(&self) -> Option<&'static str> {
        Some(STATE_KEY)
    }
    fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
        ScriptState::encode(1, self, STATE_KEY)
    }
    fn restore_state(
        &mut self,
        state: &ScriptState,
        context: &ScriptRestoreContext<'_>,
    ) -> Result<(), ScriptStateError> {
        *self = state.decode(1, STATE_KEY)?;
        for enemy in &mut self.enemies {
            // A creature destroyed in the final effect batch before saving
            // has no saved entity. Leave a tombstone to account for its death
            // once on the next update, using its last observed position.
            enemy.id = context
                .remap_entity(enemy.id)
                .map(|id| id.inner())
                .unwrap_or(0);
        }
        self.containment.remap(context);
        self.next_status = 0.0;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn tick(director: &mut HordeDirector, world: &World, seconds: f32) -> Effect {
        director.update(
            EntityId::dead(),
            world,
            &PhysicsWorld::new(),
            &Time {
                elapsed: Duration::from_secs_f32(seconds),
                total: Duration::ZERO,
            },
        )
    }

    /// Each wave opens with the centered card, not just a status line.
    #[test]
    fn a_wave_opens_with_its_own_banner() {
        let world = World::new();
        let mut director = HordeDirector {
            initialized: true,
            wave: 2,
            ..Default::default()
        };
        fn banner(effect: Effect) -> Option<(String, Duration)> {
            match effect {
                Effect::ShowBanner { text, duration } => Some((text, duration)),
                Effect::Multiple(effects) => effects.into_iter().find_map(banner),
                _ => None,
            }
        }
        let (text, duration) = banner(director.start_wave(&world)).expect("a wave card");
        assert_eq!(text, "Wave 3");
        assert_eq!(duration, WAVE_CARD_DURATION);
    }

    #[test]
    fn wave_jump_clears_attackers_without_awarding_skipped_waves() {
        let mut world = World::new();
        world.add_unique(PlayerLifeState::Alive);
        let enemy = world.add_entity((PropEcoType(ECOLOGY + 1), PropHitPoints { hit_points: 20 }));
        let inventory = world.add_entity((PropHitPoints { hit_points: 1 },));
        let mut director = HordeDirector {
            initialized: true,
            wave: 1,
            enemies: vec![Enemy {
                id: enemy.inner(),
                position: [0.0; 3],
            }],
            ..Default::default()
        };
        for wave in [9, 3, 100] {
            let effects = Effect::flatten(vec![director.jump_to_wave(&world, wave)]);
            assert_eq!(director.wave, wave);
            assert_eq!(director.phase, Phase::Assault);
            assert_eq!(director.spawned, 0);
            assert!(director.enemies.is_empty());
            assert!(
                effects.iter().any(
                    |e| matches!(e, Effect::DestroyEntity { entity_id } if *entity_id == enemy)
                )
            );
            assert!(
                !effects
                    .iter()
                    .any(|e| matches!(e, Effect::AwardXP { .. } | Effect::AwardNanites { .. }))
            );
            assert!(!effects.iter().any(
                |e| matches!(e, Effect::DestroyEntity { entity_id } if *entity_id == inventory)
            ));
        }
    }

    #[test]
    fn wave_jump_refuses_dead_player_and_unrelated_mission() {
        let world = World::new();
        let mut director = HordeDirector {
            initialized: true,
            ..Default::default()
        };
        assert!(matches!(director.jump_to_wave(&world, 9), Effect::NoEffect));
        assert!(matches!(
            wave_jump_message(&world, 9),
            Effect::ShowMessage { .. }
        ));
    }

    #[test]
    fn os_bank_unlocks_at_wave_start_and_skipped_milestones() {
        let mut world = World::new();
        for i in 0..4 {
            world.add_entity((
                PropTemplateId {
                    template_id: OS_STATIONS + i,
                },
                PropLocked(i > 0),
            ));
        }
        for (wave, count) in [(0, 0), (2, 0), (3, 1), (5, 1), (6, 2), (9, 3), (20, 3)] {
            let effects = unlock_os_stations(&world, wave);
            assert_eq!(
                effects
                    .iter()
                    .filter(|e| matches!(e, Effect::SetLocked { locked: false, .. }))
                    .count(),
                count
            );
        }
    }

    #[test]
    fn a_wave_waits_for_its_last_attacker_and_rewards_only_once() {
        let mut world = World::new();
        world.add_unique(PlayerLifeState::Alive);
        let enemy = world.add_entity((
            PropEcoType(ECOLOGY + 1),
            PropHitPoints { hit_points: 15 },
            PropPosition {
                position: vec3(1.0, 2.0, 3.0),
                cell: 0,
                rotation: Quaternion::from_angle_y(Deg(0.0)),
            },
        ));
        let mut director = HordeDirector {
            initialized: true,
            quick: true,
            phase: Phase::Assault,
            wave: 1,
            spawned: 3,
            clock: 100.0,
            enemies: vec![Enemy {
                id: enemy.inner(),
                position: [1.0, 2.0, 3.0],
            }],
            ..Default::default()
        };
        tick(&mut director, &world, 1.0);
        assert_eq!(director.phase, Phase::Assault);
        world.add_component(enemy, PropHitPoints { hit_points: 0 });
        let effects = tick(&mut director, &world, 1.0);
        assert_eq!(director.phase, Phase::Rest);
        assert_eq!(director.kills, 1);
        fn rewards(effect: Effect) -> usize {
            match effect {
                Effect::AwardXP { .. } => 1,
                Effect::Multiple(effects) => effects.into_iter().map(rewards).sum(),
                _ => 0,
            }
        }
        assert_eq!(rewards(effects), 1);
        assert_eq!(rewards(tick(&mut director, &world, 1.0)), 0);
    }

    /// A dead enemy's kill effects, as the director emits them for one tick.
    fn loot_effects(corpses: usize, keep_entities: bool) -> Vec<Effect> {
        let mut world = World::new();
        world.add_unique(PlayerLifeState::Alive);
        let mut enemies = Vec::new();
        for index in 0..corpses {
            let enemy = world.add_entity((
                PropEcoType(ECOLOGY + 1),
                PropHitPoints { hit_points: 0 },
                PropPosition {
                    position: vec3(index as f32, 2.0, 3.0),
                    cell: 0,
                    rotation: Quaternion::from_angle_y(Deg(0.0)),
                },
                Links::empty(),
            ));
            enemies.push(Enemy {
                id: enemy.inner(),
                position: [index as f32, 2.0, 3.0],
            });
            if !keep_entities {
                world.delete_entity(enemy);
            }
        }
        let mut director = HordeDirector {
            initialized: true,
            quick: true,
            phase: Phase::Assault,
            wave: 1,
            spawned: 3,
            clock: 100.0,
            enemies,
            ..Default::default()
        };
        let mut out = Effect::flatten(vec![tick(&mut director, &world, 1.0)]);
        out.retain(|effect| {
            matches!(
                effect,
                Effect::CreateEntity { .. } | Effect::CreateEntityInContainer { .. }
            )
        });
        out
    }

    #[test]
    fn kill_loot_goes_into_the_corpses_own_container() {
        let loot = loot_effects(8, true);
        assert!(!loot.is_empty(), "eight kills must roll at least one drop");
        for effect in loot {
            assert!(
                matches!(effect, Effect::CreateEntityInContainer { .. }),
                "loot must be created inside the corpse, not in the world: {effect:?}"
            );
        }
    }

    #[test]
    fn kill_loot_falls_back_to_the_world_when_no_corpse_remains() {
        let loot = loot_effects(8, false);
        assert!(!loot.is_empty(), "eight kills must roll at least one drop");
        for effect in loot {
            assert!(
                matches!(effect, Effect::CreateEntity { .. }),
                "a gibbed enemy has no container, so the drop stays in the world: {effect:?}"
            );
        }
    }

    #[test]
    fn pause_and_death_cannot_start_a_wave() {
        let mut world = World::new();
        world.add_unique(PlayerLifeState::Alive);
        let mut director = HordeDirector {
            initialized: true,
            clock: 0.1,
            ..Default::default()
        };
        tick(&mut director, &world, 0.0);
        assert_eq!(director.clock, 0.1);
        *world.borrow::<UniqueViewMut<PlayerLifeState>>().unwrap() = PlayerLifeState::Dead {
            elapsed_seconds: 0.0,
        };
        tick(&mut director, &world, 1.0);
        assert_eq!(director.phase, Phase::Failed);
        assert_eq!(director.wave, 0);
    }

    #[test]
    fn completion_waits_for_explicit_endless_and_keeps_the_character() {
        let world = World::new();
        let mut director = HordeDirector {
            initialized: true,
            wave: FINAL_WAVE,
            ..Default::default()
        };
        director.clear_wave();
        assert_eq!(director.phase, Phase::Victory);
        director.start_wave(&world);
        assert_eq!(director.wave, FINAL_WAVE + 1);
        assert_eq!(director.phase, Phase::Assault);
        director.clear_wave();
        assert_eq!(director.phase, Phase::Rest);
        assert!(director.quota() > WAVE_COUNTS[9]);
    }

    #[test]
    fn corpse_cleanup_leaves_living_enemies_and_collected_items() {
        let mut world = World::new();
        let corpse = world.add_entity((PropEcoType(ECOLOGY + 1), PropHitPoints { hit_points: 0 }));
        let unlooted = world.add_entity(());
        let collected = world.add_entity(());
        let live = world.add_entity((PropEcoType(ECOLOGY + 2), PropHitPoints { hit_points: 15 }));
        let unrelated = world.add_entity((PropEcoType(10), PropHitPoints { hit_points: 0 }));
        world.add_component(
            corpse,
            Links {
                to_links: vec![ToLink {
                    to_template_id: -87,
                    to_entity_id: Some(WrappedEntityId(unlooted)),
                    link: Link::Contains(0),
                }],
            },
        );
        let removed: Vec<_> = expired_corpses(&world)
            .into_iter()
            .map(|effect| match effect {
                Effect::DestroyEntity { entity_id } => entity_id,
                _ => unreachable!(),
            })
            .collect();
        assert!(removed.contains(&corpse) && removed.contains(&unlooted));
        for retained in [collected, live, unrelated] {
            assert!(!removed.contains(&retained));
        }
    }

    #[test]
    fn saved_schedule_and_random_stream_continue_without_reseeding() {
        let mut original = HordeDirector {
            initialized: true,
            wave: 4,
            spawned: 3,
            clock: 24.0,
            phase: Phase::Assault,
            ..Default::default()
        };
        original.roll(100);
        let state = original.save_state().unwrap();
        let mut restored: HordeDirector = state.decode(1, STATE_KEY).unwrap();
        assert_eq!(restored.wave, 4);
        assert_eq!(restored.spawned, 3);
        assert_eq!(restored.clock, 24.0);
        assert_eq!(restored.phase, Phase::Assault);
        for _ in 0..20 {
            assert_eq!(original.roll(100), restored.roll(100));
        }
    }

    #[test]
    fn final_preview_initializes_once_and_uses_the_normal_final_wave() {
        let mut world = World::new();
        world.add_unique(PlayerLifeState::Alive);
        let marker = world.add_entity((PropEcoType(2),));
        let mut director = HordeDirector::default();
        let physics = PhysicsWorld::new();
        let time = Time {
            elapsed: std::time::Duration::from_secs_f32(0.1),
            total: std::time::Duration::from_secs_f32(0.1),
        };
        director.update(marker, &world, &physics, &time);
        assert_eq!(director.wave, FINAL_WAVE - 1);
        director.start_wave(&world);
        let state = director.save_state().unwrap();
        let restored: HordeDirector = state.decode(1, STATE_KEY).unwrap();
        assert!(restored.initialized);
        assert_eq!(restored.wave, FINAL_WAVE);
        assert_eq!(restored.quota(), 38);
        assert_eq!(asset_mission("earth_horde_final"), "earth.mis");
    }

    #[test]
    fn fifteen_live_attackers_block_spawning_until_one_dies() {
        let mut world = World::new();
        let player = world.add_entity(());
        world.add_unique(PlayerLifeState::Alive);
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::from_angle_y(Deg(0.0)),
            entity_id: player,
            inventory_entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
        });
        let position = PropPosition {
            position: vec3(20.0, 0.0, 0.0),
            cell: 0,
            rotation: Quaternion::from_angle_y(Deg(0.0)),
        };
        world.add_entity((
            PropTemplateId {
                template_id: MARKER_START,
            },
            position.clone(),
        ));
        let mut director = HordeDirector {
            initialized: true,
            wave: 10,
            phase: Phase::Assault,
            next_spawn: 0.0,
            ..Default::default()
        };
        for _ in 0..15 {
            let id = world.add_entity((
                PropEcoType(ECOLOGY + 10),
                PropHitPoints { hit_points: 10 },
                position.clone(),
            ));
            director.enemies.push(Enemy {
                id: id.inner(),
                position: position.position.into(),
            });
        }
        director.spawned = 15;
        fn spawns(effect: Effect) -> usize {
            match effect {
                Effect::SpawnEcologyEntity { .. } => 1,
                Effect::Multiple(effects) => effects.into_iter().map(spawns).sum(),
                _ => 0,
            }
        }
        assert_eq!(spawns(tick(&mut director, &world, 0.1)), 0);
        let first = EntityId::from_inner(director.enemies[0].id).unwrap();
        world.add_component(first, PropHitPoints { hit_points: 0 });
        assert_eq!(spawns(tick(&mut director, &world, 0.1)), 1);
    }

    #[test]
    fn every_wave_guarantees_returning_types_and_delays_small_spiders() {
        let mut previous = Vec::new();
        for wave in 1..=FINAL_WAVE + 2 {
            let mut director = HordeDirector {
                wave,
                ..Default::default()
            };
            let roster = director.enemy_roster();
            assert!(director.quota() as usize >= roster.len());
            assert_eq!(&roster[..2], &["OG-Pipe", "OG-Shotgun"]);
            for name in &previous {
                assert!(roster.contains(name), "wave {wave} lost {name}");
            }
            assert_eq!(roster.contains(&"Baby Arachnid"), wave >= 5);
            assert_eq!(roster.contains(&"Maintenance"), wave >= 2);
            assert_eq!(roster.contains(&"Blue Monkey"), wave >= 3);
            assert_eq!(roster.contains(&"Red Monkey"), wave >= 6);
            assert_eq!(roster.contains(&"Assault"), wave >= 8);
            assert_eq!(roster.contains(&"Protocol Droid"), wave >= 2);
            for (index, name) in roster.iter().enumerate() {
                director.spawned = index as u32;
                assert_eq!(director.next_enemy(), *name);
            }
            director.spawned = roster.len() as u32;
            for _ in 0..100 {
                assert!(roster.contains(&director.next_enemy()));
            }
            previous = roster;
        }
    }

    #[test]
    fn endless_keeps_growing_past_the_old_cap_and_favors_heavy_reinforcements() {
        let mut prior = HordeDirector {
            wave: FINAL_WAVE,
            ..Default::default()
        };
        for wave in [11, 12, 108, 109, 110, 1000] {
            let next = HordeDirector {
                wave,
                ..Default::default()
            };
            assert!(next.quota() > prior.quota());
            assert!(next.assault_seconds() > prior.assault_seconds());
            prior = next;
        }
        let heavy_count = |wave| {
            let mut director = HordeDirector {
                wave,
                spawned: 100,
                ..Default::default()
            };
            (0..2000)
                .filter(|_| {
                    let name = director.next_enemy();
                    assert_ne!(
                        name, "SHODAN",
                        "avatar is a guaranteed encounter, not random filler"
                    );
                    ENEMY_INTRODUCTIONS
                        .iter()
                        .any(|(candidate, debut)| *candidate == name && *debut >= 6)
                })
                .count()
        };
        assert!(heavy_count(42) > heavy_count(10) * 2);
    }

    #[test]
    fn full_run_has_twenty_six_minutes_of_minimum_scheduled_time() {
        let mut director = HordeDirector::default();
        let mut seconds = FINAL_WAVE as f32 * director.rest_seconds();
        for wave in 1..=FINAL_WAVE {
            director.wave = wave;
            seconds += director.assault_seconds();
        }
        assert_eq!(seconds, 26.0 * 60.0);
        assert_eq!(asset_mission("EARTH_HORDE"), "earth.mis");
        assert_eq!(asset_mission("medsci1.mis"), "medsci1.mis");
    }
    fn music_changes(effect: Effect) -> Vec<Option<String>> {
        match effect {
            Effect::SetBackgroundMusic { song } => vec![song],
            Effect::Multiple(effects) => effects.into_iter().flat_map(music_changes).collect(),
            _ => vec![],
        }
    }

    #[test]
    fn waves_rotate_level_songs_and_rest_stops_music() {
        let mut director = HordeDirector::default();
        let world = World::new();
        for wave in 1..=WAVE_SONGS.len() * 2 {
            assert_eq!(
                music_changes(director.start_wave(&world)),
                vec![Some(WAVE_SONGS[(wave - 1) % WAVE_SONGS.len()].to_owned())]
            );
            assert_eq!(music_changes(director.clear_wave()), vec![None]);
        }
    }

    #[test]
    fn music_sync_is_transient_and_restored_from_the_saved_wave_and_phase() {
        for phase in [Phase::Assault, Phase::Rest, Phase::Victory, Phase::Failed] {
            let original = HordeDirector {
                phase,
                initialized: true,
                next_spawn: 100.0,
                next_status: 100.0,
                wave: 17,
                music_synced: true,
                ..Default::default()
            };
            let saved = serde_json::to_value(&original).unwrap();
            assert!(saved.get("music_synced").is_none());
            let mut restored: HordeDirector = serde_json::from_value(saved).unwrap();
            assert!(!restored.music_synced);
            assert_eq!(
                music_changes(restored.music()),
                music_changes(original.music())
            );
            let world = World::new();
            assert_eq!(
                music_changes(tick(&mut restored, &world, 0.01)),
                music_changes(original.music())
            );
            assert!(music_changes(tick(&mut restored, &world, 0.01)).is_empty());
        }
    }
}
