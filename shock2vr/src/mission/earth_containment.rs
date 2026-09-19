//! The Earth experiment's renewable containment loop. Growth uses Hydro art;
//! all runtime mutations are effects returned by the saved horde director.
use std::{collections::HashMap, sync::LazyLock};

use cgmath::{Deg, InnerSpace, Quaternion, Rotation3, vec3};
use dark::properties::*;
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Get, IntoIter, IntoWithId, UniqueView, View, World};

use super::PlayerInfo;
use crate::scripts::{Effect, Message, MessagePayload, ScriptRestoreContext};

const STATIONS: i32 = 60_200;
const PATCHES: i32 = 61_000;
const PATCH_STRIDE: i32 = 1_000;
const EGG_SITES: i32 = 60_250;
// Below the wave tags (60_001 onward), including in endless play.
const ECOLOGY: i32 = 59_000;
const MAX_DENSITY: f32 = 6.0;
const EGG_SITE_COUNT: usize = 8;
const DENSE: f32 = 4.0;
const MAX_HATCHLINGS: usize = 24;
const NAMES: [&str; 3] = ["SUBWAY", "STREET", "UPSTAIRS"];
const CIRCULATORS: [[f32; 3]; 3] = [[0.0, 2.8, 11.75], [21.0, 22.0, 31.95], [14.5, 24.4, 50.0]];
// The first four sites concentrate traps around services and their approaches;
// subsequent pods reach farther along travel routes as density raises the pod cap.
const EGG_POINTS: [[[f32; 3]; EGG_SITE_COUNT]; 3] = [
    [
        [0.0, 1.6, 4.0],
        [0.0, 1.6, 9.0],
        [21.0, 1.6, 4.0],
        [21.0, 1.6, 9.0],
        [-6.0, 1.6, 4.0],
        [27.0, 1.6, 4.0],
        [5.0, 1.6, 4.0],
        [16.0, 1.6, 4.0],
    ],
    [
        [0.0, 20.8, 30.0],
        [4.0, 20.8, 30.0],
        [21.0, 20.8, 30.0],
        [11.6, 20.8, 29.0],
        [-8.0, 20.6, 24.0],
        [20.0, 20.6, 24.0],
        [28.0, 20.6, 24.0],
        [40.0, 20.6, 24.0],
    ],
    [
        [10.0, 23.2, 44.0],
        [10.0, 23.2, 46.0],
        [10.0, 23.2, 55.0],
        [10.0, 23.2, 57.0],
        [12.5, 23.2, 50.0],
        [11.6, 23.2, 42.0],
        [11.6, 23.2, 52.0],
        [13.0, 23.2, 54.0],
    ],
];

#[derive(Clone, Copy, Deserialize)]
struct GrowthSurface {
    position: [f32; 3],
    normal: [f32; 3],
}
struct GrowthPatch {
    surface: GrowthSurface,
    onset: f32,
}
impl GrowthPatch {
    fn alpha(&self, density: f32) -> f32 {
        (density - self.onset).clamp(0.0, 1.0)
    }
}

// Authored from world-ray and same-zone navigation probes. These are actual
// surfaces in the accessible arena, not a rectangular carpet through walls.
// Distance from service-area seeds orders the growth front: nearby patches
// establish first; overlapping neighbors and higher walls follow. Lowering
// density during recovery reverses that same front.
static GROWTH: LazyLock<[Vec<GrowthPatch>; 3]> = LazyLock::new(|| {
    let surfaces: [Vec<GrowthSurface>; 3] =
        serde_json::from_str(include_str!("earth_containment_growth.json"))
            .expect("authored Earth containment surfaces");
    std::array::from_fn(|zone| {
        let distances: Vec<f32> = surfaces[zone]
            .iter()
            .map(|surface| {
                EGG_POINTS[zone][..4]
                    .iter()
                    .map(|seed| {
                        let floor_seed = vec3(seed[0], seed[1] - 0.73, seed[2]);
                        (cgmath::Vector3::from(surface.position) - floor_seed).magnitude()
                    })
                    .fold(f32::INFINITY, f32::min)
            })
            .collect();
        let furthest = distances.iter().copied().fold(1.0_f32, f32::max);
        surfaces[zone]
            .iter()
            .zip(distances)
            .map(|(surface, distance)| GrowthPatch {
                surface: *surface,
                onset: (MAX_DENSITY - 1.0) * distance / furthest,
            })
            .collect()
    })
});

pub(super) fn is_containment_type(tag: i32) -> bool {
    (ECOLOGY..ECOLOGY + 3).contains(&tag)
}

pub(super) struct Population {
    stations: Vec<EntityId>,
    patches: Vec<(EntityId, usize, usize)>,
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
        for (index, patch) in GROWTH[zone].iter().enumerate() {
            let entity = add(
                PATCHES + zone as i32 * PATCH_STRIDE + index as i32,
                [-2499, -2500, -2501][index % 3],
                &format!("{} annelid growth", NAMES[zone]),
                patch.surface.position,
                0.0,
            );
            result.patches.push((entity, zone, index));
        }
        for (site, position) in EGG_POINTS[zone].iter().enumerate() {
            result.markers.push(add(
                EGG_SITES + (zone * EGG_SITE_COUNT + site) as i32,
                -327,
                "Containment egg site",
                *position,
                0.0,
            ));
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
    for (patch, zone, index) in population.patches {
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
                PropScale(if GROWTH[zone][index].surface.normal[1] > 0.7 {
                    vec3(2.2, 1.0, 2.2)
                } else {
                    vec3(1.6, 1.0, 1.6)
                }),
            ),
        );
        let mut pos = world
            .borrow::<View<PropPosition>>()
            .unwrap()
            .get(patch)
            .unwrap()
            .clone();
        let normal = cgmath::Vector3::from(GROWTH[zone][index].surface.normal).normalize();
        pos.rotation = Quaternion::from_arc(cgmath::Vector3::unit_y(), normal, None)
            * Quaternion::from_angle_y(Deg(((index * 137 + zone * 53) % 360) as f32));
        world.add_component(patch, pos);
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
    fn stage(&self, growth_allowed: bool) -> u8 {
        if self.protection > 30.0 {
            0
        } else if self.protection > 0.0 {
            1
        } else if !growth_allowed {
            4
        } else if self.density < DENSE {
            2
        } else {
            3
        }
    }
    fn advance(&mut self, dt: f32, active: bool, quick: bool, growth_allowed: bool) {
        let scale = if quick { 10.0 } else { 1.0 };
        if self.protection > 0.0 {
            // Replenishment clears accumulated patches even during rest;
            // protection and deterioration only tick during combat.
            self.density = (self.density - dt / 2.0).max(0.0);
            if active {
                self.protection = (self.protection - dt * scale).max(0.0);
            }
        } else if active && growth_allowed {
            let seconds = crate::dev_params::get(crate::dev_params::HORDE_GROWTH_SECONDS);
            self.density = (self.density + dt * scale * MAX_DENSITY / seconds).min(MAX_DENSITY);
        }
        if active && growth_allowed && self.protection <= 0.0 && self.density >= DENSE {
            self.next_egg -= dt * scale;
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Containment {
    zones: [Zone; 3],
    // Previous gate, so unlocking emits a status even without a density change.
    growth_allowed: bool,
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
            growth_allowed: false,
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
        wave: u32,
    ) -> Vec<Effect> {
        let growth_allowed =
            quick || wave >= crate::dev_params::get(crate::dev_params::HORDE_GROWTH_WAVE) as u32;
        let mut effects = vec![];
        for (index, zone) in self.zones.iter_mut().enumerate() {
            let before = zone.stage(self.growth_allowed);
            zone.advance(dt, active, quick, growth_allowed);
            if before != zone.stage(growth_allowed) {
                let status = [
                    "PROTECTED",
                    "Toxin-A LOW",
                    "GROWTH SPREADING",
                    "DENSE GROWTH - EGGS",
                    "GROWTH DORMANT",
                ][zone.stage(growth_allowed) as usize];
                effects.push(Effect::ShowMessage {
                    text: format!("{}: {status}", NAMES[index]),
                });
            }
        }
        self.growth_allowed = growth_allowed;
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
                for (patch, growth) in GROWTH[index].iter().enumerate() {
                    if let Some(entity_id) =
                        entities.get(&(PATCHES + index as i32 * PATCH_STRIDE + patch as i32))
                    {
                        effects.push(Effect::SetRenderAlpha {
                            entity_id: *entity_id,
                            alpha: growth.alpha(zone.density),
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
                } else if hatchlings < MAX_HATCHLINGS
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
            // More of the surveyed routes become traps as growth thickens.
            let (egg_limit, egg_interval) = if zone.density >= MAX_DENSITY {
                (EGG_SITE_COUNT, 15.0)
            } else if zone.density >= DENSE + 1.0 {
                (6, 30.0)
            } else {
                (4, 45.0)
            };
            if active
                && growth_allowed
                && zone.density >= DENSE
                && zone.protection <= 0.0
                && zone.next_egg <= 0.0
                && eggs[index].len() < egg_limit
                && hatchlings < MAX_HATCHLINGS
            {
                // Never stack pods on top of one another at an occupied site.
                for offset in 0..EGG_SITE_COUNT {
                    let site = (zone.next_site + offset) % EGG_SITE_COUNT;
                    let Some(marker) =
                        entities.get(&(EGG_SITES + (index * EGG_SITE_COUNT + site) as i32))
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
                    zone.next_site = (site + 1) % EGG_SITE_COUNT;
                    zone.next_egg = egg_interval;
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
        for site in 0..EGG_SITE_COUNT as i32 {
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
    fn surveyed_growth_spreads_from_services_and_clears_in_reverse() {
        for patches in GROWTH.iter() {
            assert!(patches.len() < PATCH_STRIDE as usize);
            let mut early = 0;
            let mut late = 0;
            for patch in patches {
                assert!(patch.surface.position.iter().all(|v| v.is_finite()));
                let normal = cgmath::Vector3::from(patch.surface.normal);
                assert!((normal.magnitude() - 1.0).abs() < 0.01);
                assert_eq!(patch.alpha(0.0), 0.0);
                assert_eq!(patch.alpha(MAX_DENSITY), 1.0);
                if patch.alpha(1.0) > 0.0 {
                    early += 1;
                }
                if patch.alpha(DENSE) == 0.0 {
                    late += 1;
                }
                assert!(patch.alpha(2.0) <= patch.alpha(DENSE));
            }
            assert!(early > 0, "service approaches grow first");
            assert!(late > 0, "distant routes remain a later growth stage");
        }
    }

    #[test]
    fn slow_opening_waves_cannot_grow_or_produce_pods() {
        let (world, _) = fixture();
        let mut state = Containment::default();
        for wave in 1..=3 {
            for _ in 0..600 {
                assert_eq!(egg_spawns(&state.update(&world, 1.0, true, false, wave)), 0);
            }
            assert!(state.zones.iter().all(|zone| zone.density == 0.0));
        }
        assert!(state.zones.iter().all(|zone| zone.protection == 0.0));
        state.update(&world, 90.0, true, false, 4);
        assert_eq!(state.zones[0].density, 3.0);
        assert_eq!(egg_spawns(&state.update(&world, 30.0, true, false, 4)), 1);
        assert_eq!(state.zones[0].density, DENSE);
    }

    #[test]
    fn wave_gate_freezes_existing_growth_and_announces_unlock_once_after_load() {
        let (world, _) = fixture();
        let mut state = Containment::default();
        state.zones[0].protection = 0.0;
        state.zones[0].density = DENSE;
        state.zones[0].next_egg = 10.0;
        assert_eq!(egg_spawns(&state.update(&world, 60.0, true, false, 3)), 0);
        assert_eq!(state.zones[0].density, DENSE);
        assert_eq!(state.zones[0].next_egg, 10.0);
        let mut loaded: Containment =
            serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        let unlocked = loaded.update(&world, 1.0, true, false, 4);
        assert!(unlocked.iter().any(|effect| matches!(effect,
            Effect::ShowMessage { text } if text == "SUBWAY: DENSE GROWTH - EGGS")));
        assert_eq!(loaded.zones[0].next_egg, 9.0);
        let next = loaded.update(&world, 1.0, true, false, 4);
        assert!(!next.iter().any(|effect| matches!(effect,
            Effect::ShowMessage { text } if text == "SUBWAY: DENSE GROWTH - EGGS")));
    }

    #[test]
    fn diagnostic_mode_bypasses_the_wave_gate() {
        let (world, _) = fixture();
        let mut state = Containment::default();
        state.zones[0].protection = 0.0;
        state.update(&world, 9.0, true, true, 1);
        assert_eq!(state.zones[0].density, 3.0);
    }

    #[test]
    fn grace_is_staggered_and_rest_does_not_spend_it() {
        let mut state = Containment::default();
        assert_eq!(
            state.zones.each_ref().map(|z| z.protection),
            [180.0, 210.0, 240.0]
        );
        for zone in &mut state.zones {
            zone.advance(120.0, false, false, true);
            assert_eq!(zone.density, 0.0);
        }
        assert_eq!(state.zones[0].protection, 180.0);
        state.zones[0].advance(179.0, true, false, true);
        assert_eq!(state.zones[0].density, 0.0);
        assert_eq!(state.zones[0].stage(true), 1);
        state.zones[0].advance(1.0, true, false, true);
        state.zones[0].advance(120.0, true, false, true);
        assert_eq!(state.zones[0].density, DENSE);
        state.zones[0].advance(120.0, false, false, true);
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
        state.zones[0].advance(12.0, false, false, true);
        assert_eq!(state.zones[0].density, 0.0);
        assert_eq!(state.zones[0].protection, 180.0);
    }

    #[test]
    fn eggs_require_density_and_combat_and_hatchlings_have_their_own_limit() {
        let (mut world, _) = fixture();
        let mut state = Containment::default();
        state.zones[0].protection = 0.0;
        assert_eq!(egg_spawns(&state.update(&world, 1.0, true, false, 4)), 0);
        state.zones[0].density = 6.0;
        assert_eq!(egg_spawns(&state.update(&world, 1.0, false, false, 4)), 0);
        // Fifteen ordinary wave enemies do not consume containment capacity.
        for _ in 0..15 {
            world.add_entity((
                PropEcoType(60_001),
                PropHitPoints { hit_points: 10 },
                PropAI("Grub".into()),
            ));
        }
        assert_eq!(egg_spawns(&state.update(&world, 1.0, true, false, 4)), 1);
        for _ in 0..MAX_HATCHLINGS {
            world.add_entity((
                PropEcoType(ECOLOGY),
                PropHitPoints { hit_points: 10 },
                PropAI("Grub".into()),
            ));
        }
        state.zones[0].next_egg = 0.0;
        assert_eq!(egg_spawns(&state.update(&world, 1.0, true, false, 4)), 0);
    }

    #[test]
    fn successive_pods_reach_route_sites_then_wrap_back_to_services() {
        let (world, _) = fixture();
        let mut state = Containment::default();
        state.zones[0].protection = 0.0;
        state.zones[0].density = MAX_DENSITY;
        // Previously spawned pods have been destroyed/cleared from the world.
        // All eight authored sites must be visited before returning to services.
        for expected in (0..EGG_SITE_COUNT).chain(0..1) {
            state.zones[0].next_egg = 0.0;
            let effects = state.update(&world, 1.0, true, false, 4);
            let markers = world.borrow::<View<PropTemplateId>>().unwrap();
            let sites: Vec<_> = effects
                .iter()
                .filter_map(|effect| {
                    if let Effect::SpawnEcologyEntity { spawn_point, .. } = effect {
                        Some(markers.get(*spawn_point).unwrap().template_id)
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(sites, vec![EGG_SITES + expected as i32]);
        }
    }

    #[test]
    fn thicker_growth_fills_more_sites_and_shortens_spawn_intervals() {
        for (density, cap, interval) in [(4.0, 4, 45.0), (5.0, 6, 30.0), (6.0, 8, 15.0)] {
            let (mut world, _) = fixture();
            let mut state = Containment::default();
            state.zones[0].protection = 0.0;
            state.zones[0].density = density;
            // Fill the cap using real spawn effects, keeping the player far away.
            for site in 0..cap {
                state.zones[0].next_egg = 0.0;
                let effects = state.update(&world, 0.5, true, false, 4);
                assert_eq!(egg_spawns(&effects), 1);
                assert_eq!(state.zones[0].next_egg, interval);
                let spawn_point = effects
                    .iter()
                    .find_map(|effect| match effect {
                        Effect::SpawnEcologyEntity { spawn_point, .. } => Some(*spawn_point),
                        _ => None,
                    })
                    .unwrap();
                let position = world
                    .borrow::<View<PropPosition>>()
                    .unwrap()
                    .get(spawn_point)
                    .unwrap()
                    .clone();
                assert_eq!(position.position.x, site as f32 * 4.0);
                world.add_entity((
                    PropEcoType(ECOLOGY),
                    PropModelName("eggcl".into()),
                    position,
                ));
                assert_eq!(egg_spawns(&state.update(&world, 0.5, true, false, 4)), 0);
            }
            state.zones[0].next_egg = 0.0;
            assert_eq!(egg_spawns(&state.update(&world, 0.5, true, false, 4)), 0);
        }
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
        state.zones[0].density = DENSE;
        assert_eq!(egg_spawns(&state.update(&world, 1.0, true, false, 4)), 0);
        state.replenish(&world, station);
        assert!(
            !state
                .update(&world, 1.0, true, false, 4)
                .iter()
                .any(|e| matches!(e, Effect::DestroyEntity { .. }))
        );
    }

    #[test]
    fn rest_hatches_existing_traps_but_reserves_capacity_and_pauses_production() {
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
        state.zones[0].protection = 0.0;
        state.zones[0].density = DENSE;
        let effects = state.update(&world, 1.0, false, false, 4);
        assert_eq!(egg_spawns(&effects), 0);
        assert_eq!(state.zones[0].density, DENSE);
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
        state.update(&world, 179.0, true, false, 4);
        let saved = serde_json::to_string(&state).unwrap();
        let mut loaded: Containment = serde_json::from_str(&saved).unwrap();
        loaded.update(&world, 1.0, true, false, 4);
        assert_eq!(loaded.zones[0].protection, 0.0);
        assert_eq!(loaded.zones[1].protection, 30.0);
    }
}
