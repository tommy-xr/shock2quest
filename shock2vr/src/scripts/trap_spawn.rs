use std::cmp::Ordering;

use cgmath::{InnerSpace, point3};
use dark::{
    SCALE_FACTOR,
    properties::{Link, Links, PropEcoType, PropHitPoints, PropPosition, PropSpawn, SpawnFlags},
};
use rand::Rng;
use shipyard::{EntitiesView, EntityId, Get, UniqueView, View, ViewMut, World};

use crate::{
    mission::mission_core::PlayerInfo,
    physics::{InternalCollisionGroups, PhysicsWorld},
};

use super::{Effect, MessagePayload, Script};

const PLAYER_CLEARANCE: f32 = 30.0 / SCALE_FACTOR;

pub struct TrapSpawn;

impl TrapSpawn {
    pub fn new() -> Self {
        Self
    }

    fn choose_object(spawn: &PropSpawn) -> Option<String> {
        let total: i32 = spawn
            .object_names
            .iter()
            .zip(spawn.odds)
            .filter(|(name, odds)| !name.is_empty() && *odds > 0)
            .map(|(_, odds)| odds)
            .sum();
        if total <= 0 {
            return None;
        }

        let roll = rand::thread_rng().gen_range(0..total);
        let mut cumulative = 0;
        spawn
            .object_names
            .iter()
            .zip(spawn.odds)
            .find_map(|(name, odds)| {
                if name.is_empty() || odds <= 0 {
                    return None;
                }
                cumulative += odds;
                (roll < cumulative).then(|| name.clone())
            })
    }

    fn has_live_spawn(world: &World, marker: EntityId) -> bool {
        let Ok(links) = world.borrow::<View<Links>>() else {
            return false;
        };
        let Ok(entities) = world.borrow::<EntitiesView>() else {
            return false;
        };
        let hit_points = world.borrow::<View<PropHitPoints>>().ok();
        links.get(marker).is_ok_and(|marker_links| {
            marker_links.to_links.iter().any(|link| {
                link.link == Link::Spawned
                    && link.to_entity_id.is_some_and(|target| {
                        entities.is_alive(target.0)
                            && hit_points
                                .as_ref()
                                .and_then(|hit_points| hit_points.get(target.0).ok())
                                .is_none_or(|hit_points| hit_points.hit_points > 0)
                    })
            })
        })
    }

    fn valid_marker(
        world: &World,
        marker: EntityId,
        flags: SpawnFlags,
        player_position: cgmath::Vector3<f32>,
    ) -> bool {
        if flags.contains(SpawnFlags::POP_LIMIT) && Self::has_live_spawn(world, marker) {
            return false;
        }
        let positions = world.borrow::<View<PropPosition>>().unwrap();
        let Ok(position) = positions.get(marker) else {
            return false;
        };
        if flags.contains(SpawnFlags::PLAYER_DISTANCE) {
            let dx = position.position.x - player_position.x;
            let dz = position.position.z - player_position.z;
            if dx * dx + dz * dz < PLAYER_CLEARANCE * PLAYER_CLEARANCE {
                return false;
            }
        }
        true
    }

    fn choose_marker(
        world: &World,
        physics: &PhysicsWorld,
        generator: EntityId,
        flags: SpawnFlags,
    ) -> Option<EntityId> {
        let player_position = world.borrow::<UniqueView<PlayerInfo>>().ok()?.pos;
        let links = world.borrow::<View<Links>>().ok()?;
        let mut candidates = Vec::new();
        if flags.contains(SpawnFlags::SELF_MARKER)
            && Self::valid_marker(world, generator, flags, player_position)
        {
            candidates.push(generator);
        }
        if let Ok(generator_links) = links.get(generator) {
            candidates.extend(
                generator_links
                    .to_links
                    .iter()
                    .filter(|link| link.link == Link::SpawnPoint)
                    .filter_map(|link| link.to_entity_id.map(|target| target.0))
                    .filter(|marker| Self::valid_marker(world, *marker, flags, player_position)),
            );
        }
        if candidates.is_empty() {
            return None;
        }

        let positions = world.borrow::<View<PropPosition>>().unwrap();
        if flags.contains(SpawnFlags::FARTHEST) {
            return candidates.into_iter().max_by(|left, right| {
                let distance = |entity| {
                    positions
                        .get(entity)
                        .map(|position| {
                            let dx = position.position.x - player_position.x;
                            let dz = position.position.z - player_position.z;
                            dx * dx + dz * dz
                        })
                        .unwrap_or(0.0)
                };
                distance(*left)
                    .partial_cmp(&distance(*right))
                    .unwrap_or(Ordering::Equal)
            });
        }

        let marker = candidates[rand::thread_rng().gen_range(0..candidates.len())];
        if flags.contains(SpawnFlags::RAYCAST) {
            let marker_position = positions.get(marker).ok()?.position;
            let delta = player_position - marker_position;
            let distance = delta.magnitude();
            if distance == 0.0
                || physics
                    .ray_cast2(
                        point3(0.0, 0.0, 0.0) + marker_position,
                        delta / distance,
                        distance,
                        InternalCollisionGroups::WORLD,
                        Some(marker),
                        true,
                    )
                    .is_none()
            {
                return None;
            }
        }
        Some(marker)
    }
}

impl Script for TrapSpawn {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        if !matches!(msg, MessagePayload::TurnOn { .. }) {
            return Effect::NoEffect;
        }

        let spawn = {
            let mut spawns = world.borrow::<ViewMut<PropSpawn>>().unwrap();
            let Ok(spawn) = (&mut spawns).get(entity_id) else {
                return Effect::NoEffect;
            };
            let current = spawn.clone();
            if spawn.supply > 0 {
                spawn.supply = if spawn.supply == 1 {
                    -1
                } else {
                    spawn.supply - 1
                };
            }
            current
        };
        if spawn.supply == -1 {
            return Effect::NoEffect;
        }
        let Some(template_name) = Self::choose_object(&spawn) else {
            return Effect::NoEffect;
        };
        let Some(spawn_point) = Self::choose_marker(world, physics, entity_id, spawn.flags) else {
            return Effect::NoEffect;
        };
        let ecology_type = world
            .borrow::<View<PropEcoType>>()
            .ok()
            .and_then(|types| types.get(entity_id).ok().map(|ecology_type| ecology_type.0));

        Effect::SpawnEcologyEntity {
            template_name,
            spawn_point,
            ecology_type,
            goto_player: spawn.flags.contains(SpawnFlags::GOTO_LOCATION),
        }
    }
}
