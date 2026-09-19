//! Small wave pickups and a bounded, persistent supply-cache objective.
use std::collections::HashSet;

use cgmath::{Deg, InnerSpace, Matrix4, Point3, Quaternion, Rotation3, SquareMatrix};
use dark::properties::{Link, Links, PropEcoType};
use rand::{Rng, RngCore, SeedableRng};
use serde::{Deserialize, Serialize};
use shipyard::{Get, IntoIter, IntoWithId, UniqueView, View, World};

use super::{PlayerInfo, earth_horde::SITES, entity_creator::CreateEntityOptions};
use crate::scripts::Effect;

const PICKUP: i32 = 58_000;
const CACHE: i32 = 58_001;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct SupplyDrops {
    seed: u64,
    cache_due: Option<f32>,
    pending_fill: bool,
}
impl Default for SupplyDrops {
    fn default() -> Self {
        Self {
            seed: rand::random(),
            cache_due: None,
            pending_fill: false,
        }
    }
}
impl SupplyDrops {
    fn roll(&mut self, count: usize) -> usize {
        let mut rng = rand::rngs::StdRng::seed_from_u64(self.seed);
        self.seed = rng.next_u64();
        rng.gen_range(0..count)
    }

    fn sites(world: &World) -> Vec<[f32; 3]> {
        let Ok(player) = world.borrow::<UniqueView<PlayerInfo>>() else {
            return vec![];
        };
        SITES
            .iter()
            .copied()
            .filter(|site| {
                (site[1] < 10.0) == (player.pos.y < 10.0)
                    && (cgmath::Vector3::from(*site) - player.pos).magnitude2() > 4.0 * 4.0
            })
            .collect()
    }

    pub(super) fn start_wave(&mut self, world: &World, wave: u32, quick: bool) -> Vec<Effect> {
        self.cache_due = (wave % 2 == 0).then_some(if quick { 3.0 } else { 20.0 });
        let (tags, links) = world.borrow::<(View<PropEcoType>, View<Links>)>().unwrap();
        let mut carried: HashSet<_> = links
            .iter()
            .flat_map(|links| links.to_links.iter())
            .filter(|link| matches!(link.link, Link::Contains(_)))
            .filter_map(|link| link.to_entity_id.map(|entity| entity.0))
            .collect();
        if let Ok(player) = world.borrow::<UniqueView<PlayerInfo>>() {
            carried.extend(
                [player.left_hand_entity_id, player.right_hand_entity_id]
                    .into_iter()
                    .flatten(),
            );
        }
        // Only last wave's abandoned loose supplies expire. Collected items,
        // held items, and the persistent cache's remaining contents survive.
        let mut effects: Vec<_> = tags
            .iter()
            .with_id()
            .filter(|(id, tag)| tag.0 == PICKUP && !carried.contains(id))
            .map(|(entity_id, _)| Effect::DestroyEntity { entity_id })
            .collect();
        let mut sites = Self::sites(world);
        for choices in [&[-1358, -57][..], &[-52, -87][..]] {
            if sites.is_empty() {
                break;
            }
            let index = self.roll(sites.len());
            let site = sites.swap_remove(index);
            let item = choices[self.roll(choices.len())];
            effects.push(Self::create(item, site, PICKUP));
        }
        if !effects.is_empty() {
            effects.push(Effect::ShowMessage {
                text: "Fresh supplies nearby. Uncollected loose supplies expire next wave.".into(),
            });
        }
        effects
    }

    fn create(template_id: i32, site: [f32; 3], tag: i32) -> Effect {
        Effect::CreateEntity {
            template_id,
            position: Point3::new(site[0], site[1] + 0.6, site[2]),
            orientation: Quaternion::from_angle_y(Deg(0.0)),
            root_transform: Matrix4::identity(),
            options: CreateEntityOptions {
                ecology_type: Some(tag),
                ..Default::default()
            },
        }
    }

    pub(super) fn update(&mut self, world: &World, dt: f32, active: bool) -> Vec<Effect> {
        let (tags, links) = world.borrow::<(View<PropEcoType>, View<Links>)>().unwrap();
        let cache = tags
            .iter()
            .with_id()
            .find(|(_, tag)| tag.0 == CACHE)
            .map(|(id, _)| id);
        if self.pending_fill {
            if let Some(container_entity_id) = cache {
                self.pending_fill = false;
                // Supplies every build can use, plus one progression reward.
                let bonus = [-87, -938][self.roll(2)];
                return [-1358, -52, -57, bonus]
                    .into_iter()
                    .map(|template_id| Effect::CreateEntityInContainer {
                        template_id,
                        container_entity_id,
                    })
                    .collect();
            }
            self.pending_fill = false;
            self.cache_due = Some(1.0);
            return vec![];
        }
        if let Some(entity_id) = cache {
            if links.get(entity_id).is_ok_and(|links| {
                !links
                    .to_links
                    .iter()
                    .any(|link| matches!(link.link, Link::Contains(_)))
            }) {
                return vec![Effect::DestroyEntity { entity_id }];
            }
        }
        if !active {
            return vec![];
        }
        let Some(due) = &mut self.cache_due else {
            return vec![];
        };
        *due -= dt;
        if *due > 0.0 || cache.is_some() {
            return vec![];
        }
        let sites = Self::sites(world);
        if sites.is_empty() {
            return vec![];
        }
        let site = sites[self.roll(sites.len())];
        self.cache_due = None;
        self.pending_fill = true;
        vec![
            Self::create(-941, site, CACHE),
            Effect::ShowMessage {
                text: format!(
                    "Supply cache delivered to {}. Search the TriOptimum crate.",
                    if site[1] < 10.0 {
                        "the subway"
                    } else if site[1] > 21.0 {
                        "the upstairs landing"
                    } else {
                        "the street"
                    }
                ),
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::vec3;
    use dark::properties::{ToLink, WrappedEntityId};
    fn world() -> World {
        let mut world = World::new();
        let player = world.add_entity(());
        world.add_unique(PlayerInfo {
            entity_id: player,
            inventory_entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            pos: vec3(11.6, 21.0, 24.0),
            rotation: Quaternion::from_angle_y(Deg(0.0)),
        });
        world
    }
    #[test]
    fn cache_waits_for_combat_and_fills_once_across_save() {
        let mut world = world();
        let mut drops = SupplyDrops::default();
        drops.start_wave(&world, 2, false);
        assert!(drops.update(&world, 100.0, false).is_empty());
        assert!(drops.update(&world, 19.0, true).is_empty());
        assert!(
            drops
                .update(&world, 1.0, true)
                .iter()
                .any(|effect| matches!(
                    effect,
                    Effect::CreateEntity {
                        template_id: -941,
                        ..
                    }
                ))
        );
        let cache = world.add_entity((PropEcoType(CACHE), Links { to_links: vec![] }));
        let mut loaded: SupplyDrops =
            serde_json::from_str(&serde_json::to_string(&drops).unwrap()).unwrap();
        assert_eq!(loaded.update(&world, 1.0, false).iter().filter(|effect| matches!(effect,
            Effect::CreateEntityInContainer { container_entity_id, .. } if *container_entity_id == cache)).count(), 4);
        assert!(!loaded.pending_fill);
    }
    #[test]
    fn next_wave_removes_abandoned_pickups_but_keeps_collected_ones() {
        let mut world = world();
        let abandoned = world.add_entity((PropEcoType(PICKUP),));
        let collected = world.add_entity((PropEcoType(PICKUP),));
        world.add_entity((Links {
            to_links: vec![ToLink {
                to_template_id: 0,
                to_entity_id: Some(WrappedEntityId(collected)),
                link: Link::Contains(0),
            }],
        },));
        let effects = SupplyDrops::default().start_wave(&world, 1, false);
        assert!(effects.iter().any(
            |effect| matches!(effect, Effect::DestroyEntity {entity_id} if *entity_id == abandoned)
        ));
        assert!(!effects.iter().any(
            |effect| matches!(effect, Effect::DestroyEntity {entity_id} if *entity_id == collected)
        ));
        assert_eq!(
            effects
                .iter()
                .filter(|effect| matches!(effect, Effect::CreateEntity { .. }))
                .count(),
            2
        );
    }
}
