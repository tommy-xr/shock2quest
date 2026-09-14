//! The Earth experiment's renewable containment loop. Growth uses Hydro art;
//! all runtime mutations are effects returned by the saved horde director.
use std::collections::HashMap;

use cgmath::{Deg, InnerSpace, Quaternion, Rotation3, vec3};
use dark::properties::*;
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Get, IntoIter, IntoWithId, UniqueView, View, World};

use super::PlayerInfo;
use crate::scripts::{Effect, Message, MessagePayload, ScriptRestoreContext};

const STATIONS: i32 = 60_200;
const PATCHES: i32 = 60_220;
const EGG_SITES: i32 = 60_250;
// Below the wave tags (60_001 onward), including in endless play.
const ECOLOGY: i32 = 59_000;
const PATCH_COUNT: usize = 6;
const DENSE: f32 = 4.0;
const MAX_EGGS_PER_ZONE: usize = 4;
const MAX_HATCHLINGS: usize = 24;
const NAMES: [&str; 3] = ["SUBWAY", "STREET", "UPSTAIRS"];
const CIRCULATORS: [[f32; 3]; 3] = [[0.0, 2.8, 11.75], [21.0, 22.0, 31.95], [14.5, 24.4, 50.0]];
// Four floor sites and two wall sites per zone; walls rotate the same thin
// Hydro mesh rather than stretching a floor patch into a vertical volume.
const GROWTH: [[[f32; 3]; PATCH_COUNT]; 3] = [
    [
        [0.0, 0.87, 4.0],
        [0.0, 0.87, 7.0],
        [21.0, 0.87, 4.0],
        [21.0, 0.87, 7.0],
        [20.0, 2.5, 12.73],
        [23.0, 2.5, 12.73],
    ],
    [
        [-8.0, 19.87, 24.0],
        [4.0, 19.87, 24.0],
        [20.0, 19.87, 24.0],
        [28.0, 19.87, 24.0],
        [0.0, 25.5, 33.13],
        [4.0, 25.5, 33.13],
    ],
    [
        [11.6, 22.47, 44.0],
        [10.0, 22.47, 48.0],
        [11.6, 22.47, 52.0],
        [12.0, 22.47, 56.0],
        [6.87, 24.0, 42.0],
        [6.87, 24.0, 47.8],
    ],
];

pub(super) fn is_containment_type(tag: i32) -> bool {
    (ECOLOGY..ECOLOGY + 3).contains(&tag)
}

pub(super) struct Population {
    stations: Vec<EntityId>,
    patches: Vec<EntityId>,
    markers: Vec<EntityId>,
}

pub(super) fn populate(
    add: &mut impl FnMut(i32, i32, &str, [f32; 3], f32) -> EntityId,
) -> Population {
    let mut result = Population {
        stations: vec![],
        patches: vec![],
        markers: vec![],
    };
    for zone in 0..3 {
        result.stations.push(add(
            STATIONS + zone as i32,
            -1151,
            &format!("{} Air Circulator - Toxin-A", NAMES[zone]),
            CIRCULATORS[zone],
            if zone == 2 { 90.0 } else { 0.0 },
        ));
        for patch in 0..PATCH_COUNT {
            result.patches.push(add(
                PATCHES + (zone * PATCH_COUNT + patch) as i32,
                if patch >= 4 {
                    -2499
                } else {
                    [-2499, -2500, -2501][patch % 3]
                },
                &format!("{} annelid growth", NAMES[zone]),
                GROWTH[zone][patch],
                0.0,
            ));
            if patch < 4 {
                let mut pos = GROWTH[zone][patch];
                pos[1] += 0.73; // Closed egg extends .784 below its origin.
                result.markers.push(add(
                    EGG_SITES + (zone * 4 + patch) as i32,
                    -327,
                    "Containment egg site",
                    pos,
                    0.0,
                ));
            }
        }
    }
    result
}

pub(super) fn configure(world: &mut World, population: Population, director: EntityId) {
    for station in population.stations {
        // Hydro's final-model latch is deliberately removed: this mission's
        // station is renewable. Keep the existing flat/VR consumption path.
        world.delete_component::<(PropTweqModelConfig,)>(station);
        world.add_component(
            station,
            (
                PropScripts {
                    scripts: vec!["ObjConsumeButton".into()],
                    inherits: false,
                },
                PropConsumeType("Anti-Annelid Toxin".into()),
                Links {
                    to_links: vec![ToLink {
                        to_template_id: 60_000,
                        to_entity_id: Some(WrappedEntityId(director)),
                        link: Link::SwitchLink,
                    }],
                },
                PropModelName("air_re".into()),
            ),
        );
    }
    for (index, patch) in population.patches.into_iter().enumerate() {
        world.delete_component::<(
            PropPhysType,
            PropPhysDimensions,
            PropFrobInfo,
            PropHUDSelect,
        )>(patch);
        world.add_component(
            patch,
            (
                PropScripts {
                    scripts: vec![],
                    inherits: false,
                },
                PropRenderType(RenderType::Normal),
                PropRenderAlpha(0.0),
                PropScale(if index % PATCH_COUNT >= 4 {
                    vec3(1.0, 1.0, 1.0)
                } else {
                    vec3(1.8, 1.0, 1.8)
                }),
            ),
        );
        if index % PATCH_COUNT >= 4 {
            let mut pos = world
                .borrow::<View<PropPosition>>()
                .unwrap()
                .get(patch)
                .unwrap()
                .clone();
            pos.rotation = if index / PATCH_COUNT == 2 {
                Quaternion::from_angle_z(Deg(-90.0))
            } else {
                Quaternion::from_angle_x(Deg(-90.0))
            };
            world.add_component(patch, pos);
        }
    }
    for marker in population.markers {
        world.add_component(
            marker,
            PropScripts {
                scripts: vec![],
                inherits: false,
            },
        );
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Zone {
    protection: f32,
    density: f32,
    next_egg: f32,
    next_site: usize,
    shown: i32,
}
impl Zone {
    fn stage(&self) -> u8 {
        if self.protection > 30.0 {
            0
        } else if self.protection > 0.0 {
            1
        } else if self.density < DENSE {
            2
        } else {
            3
        }
    }
    fn advance(&mut self, dt: f32, active: bool, quick: bool) {
        let scale = if quick { 10.0 } else { 1.0 };
        if self.protection > 0.0 {
            // Replenishment clears accumulated patches even during rest;
            // protection and deterioration only tick during combat.
            self.density = (self.density - dt / 2.0).max(0.0);
            if active {
                self.protection = (self.protection - dt * scale).max(0.0);
            }
        } else if active {
            self.density = (self.density + dt * scale / 15.0).min(PATCH_COUNT as f32);
        }
        if active && self.protection <= 0.0 && self.density >= DENSE {
            self.next_egg -= dt * scale;
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Containment {
    zones: [Zone; 3],
    // Open shells expire independently of waves; closed eggs remain a threat.
    shells: HashMap<u64, f32>,
    poll: f32,
}
impl Default for Containment {
    fn default() -> Self {
        Self {
            zones: std::array::from_fn(|zone| Zone {
                protection: 180.0 + zone as f32 * 30.0,
                density: 0.0,
                next_egg: 0.0,
                next_site: 0,
                shown: -1,
            }),
            shells: HashMap::new(),
            poll: 0.0,
        }
    }
}
impl Containment {
    pub(super) fn replenish(&mut self, world: &World, from: EntityId) -> Effect {
        let Ok(ids) = world.borrow::<View<PropTemplateId>>() else {
            return Effect::NoEffect;
        };
        let Ok(id) = ids.get(from) else {
            return Effect::NoEffect;
        };
        let zone = id.template_id - STATIONS;
        if !(0..3).contains(&zone) {
            return Effect::NoEffect;
        }
        let state = &mut self.zones[zone as usize];
        state.protection = 180.0;
        state.next_egg = 45.0;
        Effect::Multiple(vec![
            Effect::ChangeModel {
                entity_id: from,
                model_name: "air_re".into(),
            },
            Effect::ShowMessage {
                text: format!(
                    "{}: Toxin-A replenished. Growth receding; eggs remain.",
                    NAMES[zone as usize]
                ),
            },
        ])
    }

    pub(super) fn update(
        &mut self,
        world: &World,
        dt: f32,
        active: bool,
        quick: bool,
    ) -> Vec<Effect> {
        let mut effects = vec![];
        for (index, zone) in self.zones.iter_mut().enumerate() {
            let before = zone.stage();
            zone.advance(dt, active, quick);
            if before != zone.stage() {
                let status = [
                    "PROTECTED",
                    "Toxin-A LOW",
                    "GROWTH SPREADING",
                    "DENSE GROWTH - EGGS",
                ][zone.stage() as usize];
                effects.push(Effect::ShowMessage {
                    text: format!("{}: {status}", NAMES[index]),
                });
            }
        }
        for age in self.shells.values_mut() {
            *age += dt;
        }
        self.poll -= dt;
        if self.poll > 0.0 {
            return effects;
        }
        self.poll = 0.5;
        let Ok((ids, models, positions, tags, hp, ais)) = world.borrow::<(
            View<PropTemplateId>,
            View<PropModelName>,
            View<PropPosition>,
            View<PropEcoType>,
            View<PropHitPoints>,
            View<PropAI>,
        )>() else {
            return effects;
        };
        let entities: HashMap<_, _> = ids
            .iter()
            .with_id()
            .map(|(id, template)| (template.template_id, id))
            .collect();
        let player = world.borrow::<UniqueView<PlayerInfo>>().ok().map(|p| p.pos);
        let mut hatchlings = (&tags, &hp, &ais)
            .iter()
            .filter(|(tag, hp, _)| is_containment_type(tag.0) && hp.hit_points > 0)
            .count();
        let mut eggs: [Vec<EntityId>; 3] = Default::default();
        for (id, (tag, model)) in (&tags, &models).iter().with_id() {
            if !is_containment_type(tag.0) {
                continue;
            }
            if model.0.eq_ignore_ascii_case("eggcl") || model.0.eq_ignore_ascii_case("eggop") {
                eggs[(tag.0 - ECOLOGY) as usize].push(id);
            }
        }
        // Stable site/hatching order across saves and ECS iteration order.
        for pods in &mut eggs {
            pods.sort_by_key(|id| id.inner());
        }
        for (index, zone) in self.zones.iter_mut().enumerate() {
            if let Some(station) = entities.get(&(STATIONS + index as i32)) {
                let model = if zone.protection > 0.0 {
                    "air_re"
                } else {
                    "air_reof"
                };
                if models.get(*station).is_ok_and(|p| p.0 != model) {
                    effects.push(Effect::ChangeModel {
                        entity_id: *station,
                        model_name: model.into(),
                    });
                }
            }
            let shown = (zone.density * 10.0).round() as i32;
            if zone.shown != shown {
                for patch in 0..PATCH_COUNT {
                    if let Some(entity_id) =
                        entities.get(&(PATCHES + (index * PATCH_COUNT + patch) as i32))
                    {
                        effects.push(Effect::SetRenderAlpha {
                            entity_id: *entity_id,
                            alpha: (zone.density - patch as f32).clamp(0.0, 1.0),
                        });
                    }
                }
                zone.shown = shown;
            }
            for pod in &eggs[index] {
                if hp.get(*pod).is_ok_and(|hp| hp.hit_points <= 0) {
                    continue;
                }
                let open = models
                    .get(*pod)
                    .is_ok_and(|m| m.0.eq_ignore_ascii_case("eggop"));
                if open {
                    self.shells.entry(pod.inner()).or_insert(0.0);
                } else if active
                    && hatchlings < MAX_HATCHLINGS
                    && player.is_some_and(|player| {
                        positions
                            .get(*pod)
                            .is_ok_and(|pos| (pos.position - player).magnitude2() < 4.0 * 4.0)
                    })
                {
                    effects.push(Effect::Send {
                        msg: Message {
                            to: *pod,
                            payload: MessagePayload::TurnOn { from: *pod },
                        },
                    });
                    hatchlings += 1; // reserve births emitted by this batch
                }
            }
            if active
                && zone.density >= DENSE
                && zone.protection <= 0.0
                && zone.next_egg <= 0.0
                && eggs[index].len() < MAX_EGGS_PER_ZONE
                && hatchlings < MAX_HATCHLINGS
            {
                // Never stack pods on top of one another at an occupied site.
                for offset in 0..4 {
                    let site = (zone.next_site + offset) % 4;
                    let Some(marker) = entities.get(&(EGG_SITES + (index * 4 + site) as i32))
                    else {
                        continue;
                    };
                    let Ok(at) = positions.get(*marker) else {
                        continue;
                    };
                    if eggs[index].iter().any(|egg| {
                        positions
                            .get(*egg)
                            .is_ok_and(|p| (p.position - at.position).magnitude2() < 1.0)
                    }) {
                        continue;
                    }
                    effects.push(Effect::SpawnEcologyEntity {
                        template_name: "Grub Floor Pod".into(),
                        spawn_point: *marker,
                        ecology_type: Some(ECOLOGY + index as i32),
                        goto_player: false,
                    });
                    zone.next_site = (site + 1) % 4;
                    zone.next_egg = 45.0;
                    break;
                }
            }
        }
        self.shells.retain(|id, age| {
            let alive = models.get(EntityId::from_inner(*id).unwrap()).is_ok();
            if alive && *age >= 30.0 {
                effects.push(Effect::DestroyEntity {
                    entity_id: EntityId::from_inner(*id).unwrap(),
                });
            }
            alive && *age < 30.0
        });
        effects
    }

    pub(super) fn remap(&mut self, context: &ScriptRestoreContext<'_>) {
        self.shells = self
            .shells
            .iter()
            .filter_map(|(id, age)| {
                context
                    .remap_entity(*id)
                    .ok()
                    .map(|new| (new.inner(), *age))
            })
            .collect();
        for zone in &mut self.zones {
            zone.shown = -1;
        }
        self.poll = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (World, EntityId) {
        let mut world = World::new();
        let player = world.add_entity(());
        world.add_unique(PlayerInfo {
            entity_id: player,
            inventory_entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            pos: vec3(100.0, 100.0, 100.0),
            rotation: Quaternion::from_angle_y(Deg(0.0)),
        });
        let station = world.add_entity((PropTemplateId {
            template_id: STATIONS,
        },));
        for site in 0..4 {
            world.add_entity((
                PropTemplateId {
                    template_id: EGG_SITES + site,
                },
                PropPosition {
                    position: vec3(site as f32 * 4.0, 1.6, 0.0),
                    cell: u16::MAX,
                    rotation: Quaternion::from_angle_y(Deg(0.0)),
                },
            ));
        }
        (world, station)
    }

    fn egg_spawns(effects: &[Effect]) -> usize {
        effects
            .iter()
            .filter(|e| matches!(e, Effect::SpawnEcologyEntity { .. }))
            .count()
    }

    #[test]
    fn grace_is_staggered_and_rest_does_not_spend_it() {
        let mut state = Containment::default();
        assert_eq!(
            state.zones.each_ref().map(|z| z.protection),
            [180.0, 210.0, 240.0]
        );
        for zone in &mut state.zones {
            zone.advance(120.0, false, false);
            assert_eq!(zone.density, 0.0);
        }
        assert_eq!(state.zones[0].protection, 180.0);
        state.zones[0].advance(179.0, true, false);
        assert_eq!(state.zones[0].density, 0.0);
        assert_eq!(state.zones[0].stage(), 1);
        state.zones[0].advance(1.0, true, false);
        state.zones[0].advance(60.0, true, false);
        assert_eq!(state.zones[0].density, DENSE);
        state.zones[0].advance(120.0, false, false);
        assert_eq!(state.zones[0].density, DENSE);
    }

    #[test]
    fn toxin_restores_only_its_zone_and_clears_growth_during_rest() {
        let (world, station) = fixture();
        let mut state = Containment::default();
        for zone in &mut state.zones {
            zone.protection = 0.0;
            zone.density = 6.0;
        }
        assert!(matches!(
            state.replenish(&world, station),
            Effect::Multiple(_)
        ));
        assert_eq!(state.zones[0].protection, 180.0);
        assert_eq!(state.zones[1].protection, 0.0);
        state.zones[0].advance(12.0, false, false);
        assert_eq!(state.zones[0].density, 0.0);
        assert_eq!(state.zones[0].protection, 180.0);
    }

    #[test]
    fn eggs_require_density_and_combat_and_hatchlings_have_their_own_limit() {
        let (mut world, _) = fixture();
        let mut state = Containment::default();
        state.zones[0].protection = 0.0;
        assert_eq!(egg_spawns(&state.update(&world, 1.0, true, false)), 0);
        state.zones[0].density = 6.0;
        assert_eq!(egg_spawns(&state.update(&world, 1.0, false, false)), 0);
        // Fifteen ordinary wave enemies do not consume containment capacity.
        for _ in 0..15 {
            world.add_entity((
                PropEcoType(60_001),
                PropHitPoints { hit_points: 10 },
                PropAI("Grub".into()),
            ));
        }
        assert_eq!(egg_spawns(&state.update(&world, 1.0, true, false)), 1);
        for _ in 0..MAX_HATCHLINGS {
            world.add_entity((
                PropEcoType(ECOLOGY),
                PropHitPoints { hit_points: 10 },
                PropAI("Grub".into()),
            ));
        }
        state.zones[0].next_egg = 0.0;
        assert_eq!(egg_spawns(&state.update(&world, 1.0, true, false)), 0);
    }

    #[test]
    fn occupied_sites_do_not_stack_eggs_and_closed_eggs_survive_replenishment() {
        let (mut world, station) = fixture();
        for site in 0..4 {
            world.add_entity((
                PropEcoType(ECOLOGY),
                PropModelName("eggcl".into()),
                PropPosition {
                    position: vec3(site as f32 * 4.0, 1.6, 0.0),
                    cell: u16::MAX,
                    rotation: Quaternion::from_angle_y(Deg(0.0)),
                },
            ));
        }
        let mut state = Containment::default();
        state.zones[0].protection = 0.0;
        state.zones[0].density = 6.0;
        assert_eq!(egg_spawns(&state.update(&world, 1.0, true, false)), 0);
        state.replenish(&world, station);
        assert!(
            !state
                .update(&world, 1.0, true, false)
                .iter()
                .any(|e| matches!(e, Effect::DestroyEntity { .. }))
        );
    }

    #[test]
    fn a_hatch_batch_reserves_capacity_and_preserves_other_wave_slots() {
        let (mut world, _) = fixture();
        world
            .borrow::<shipyard::UniqueViewMut<PlayerInfo>>()
            .unwrap()
            .pos = vec3(0.0, 1.6, 0.0);
        for _ in 0..2 {
            world.add_entity((
                PropEcoType(ECOLOGY),
                PropModelName("eggcl".into()),
                PropHitPoints { hit_points: 5 },
                PropPosition {
                    position: vec3(0.0, 1.6, 0.0),
                    cell: u16::MAX,
                    rotation: Quaternion::from_angle_y(Deg(0.0)),
                },
            ));
        }
        for _ in 0..MAX_HATCHLINGS - 1 {
            world.add_entity((
                PropEcoType(ECOLOGY),
                PropHitPoints { hit_points: 10 },
                PropAI("Grub".into()),
            ));
        }
        let mut state = Containment::default();
        let effects = state.update(&world, 1.0, true, false);
        assert_eq!(
            effects
                .iter()
                .filter(|effect| matches!(
                    effect,
                    Effect::Send {
                        msg: Message {
                            payload: MessagePayload::TurnOn { .. },
                            ..
                        }
                    }
                ))
                .count(),
            1
        );
    }

    #[test]
    fn containment_timers_round_trip_without_reissuing_protection() {
        let (world, _) = fixture();
        let mut state = Containment::default();
        state.update(&world, 179.0, true, false);
        let saved = serde_json::to_string(&state).unwrap();
        let mut loaded: Containment = serde_json::from_str(&saved).unwrap();
        loaded.update(&world, 1.0, true, false);
        assert_eq!(loaded.zones[0].protection, 0.0);
        assert_eq!(loaded.zones[1].protection, 30.0);
    }
}
