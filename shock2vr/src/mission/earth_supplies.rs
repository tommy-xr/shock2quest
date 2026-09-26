//! Timed intermission supplies on both floors, using ordinary container/hack UI.
use std::collections::HashSet;

use cgmath::{Deg, InnerSpace, Matrix4, Point3, Quaternion, Rotation3, SquareMatrix};
use dark::properties::{Link, Links, PropEcoType};
use rand::{Rng, RngCore, SeedableRng};
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Get, IntoIter, IntoWithId, UniqueView, View, World};

use super::{PlayerInfo, earth_horde::SITES, entity_creator::CreateEntityOptions};
use crate::scripts::Effect;

const PICKUP: i32 = 58_000;
const CACHE: i32 = 58_001;
const CLAIM_SECONDS: f32 = 90.0;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct SupplyDrops {
    seed: u64,
    in_rest: bool,
    remaining: Option<f32>,
    pending_fill: [bool; 2],
}
impl Default for SupplyDrops {
    fn default() -> Self {
        Self {
            seed: rand::random(),
            in_rest: false,
            remaining: None,
            pending_fill: [false; 2],
        }
    }
}
impl SupplyDrops {
    fn roll(&mut self, count: usize) -> usize {
        let mut rng = rand::rngs::StdRng::seed_from_u64(self.seed);
        self.seed = rng.next_u64();
        rng.gen_range(0..count)
    }

    fn sites(world: &World, floor: usize) -> Vec<[f32; 3]> {
        let player = world.borrow::<UniqueView<PlayerInfo>>().ok();
        SITES
            .iter()
            .copied()
            .filter(|site| {
                (site[1] >= 10.0) == (floor == 1)
                    && player
                        .as_ref()
                        .is_none_or(|p| (cgmath::Vector3::from(*site) - p.pos).magnitude2() > 16.0)
            })
            .collect()
    }

    pub(super) fn start_wave(&mut self, _world: &World, _wave: u32, _quick: bool) -> Vec<Effect> {
        // A skipped rest does not cancel the remaining claim window.
        self.in_rest = false;
        vec![]
    }

    fn carried(world: &World) -> HashSet<EntityId> {
        let links = world.borrow::<View<Links>>().unwrap();
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
        carried
    }

    fn remove(world: &World, roots: Vec<EntityId>) -> Vec<Effect> {
        let links = world.borrow::<View<Links>>().unwrap();
        let mut ids = roots;
        let mut seen: HashSet<_> = ids.iter().copied().collect();
        let mut index = 0;
        while index < ids.len() {
            if let Ok(contents) = links.get(ids[index]) {
                for link in &contents.to_links {
                    if matches!(link.link, Link::Contains(_)) {
                        if let Some(child) = link.to_entity_id.filter(|child| seen.insert(child.0))
                        {
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

    fn expire(world: &World) -> Vec<Effect> {
        let carried = Self::carried(world);
        let tags = world.borrow::<View<PropEcoType>>().unwrap();
        let roots = tags
            .iter()
            .with_id()
            .filter(|(id, tag)| (PICKUP..=CACHE + 1).contains(&tag.0) && !carried.contains(id))
            .map(|(id, _)| id)
            .collect();
        Self::remove(world, roots)
    }

    fn begin_rest(&mut self, world: &World) -> Vec<Effect> {
        let mut effects = Self::expire(world);
        self.remaining = Some(CLAIM_SECONDS);
        self.pending_fill = [false; 2];
        for floor in 0..2 {
            let mut sites = Self::sites(world, floor);
            for choices in [&[-1358, -57][..], &[-52, -87][..]] {
                if sites.is_empty() {
                    break;
                }
                let index = self.roll(sites.len());
                let site = sites.swap_remove(index);
                let template = choices[self.roll(choices.len())];
                effects.push(Self::create(template, site, PICKUP));
            }
            if !sites.is_empty() {
                let site = sites[self.roll(sites.len())];
                let secured = self.roll(3) == 0;
                effects.push(Self::create(
                    if secured { -1886 } else { -941 },
                    site,
                    CACHE + floor as i32,
                ));
                self.pending_fill[floor] = true;
            }
        }
        effects.push(Effect::ShowMessage { text: "Supplies on BOTH floors! Search crates; security crates need hacking. Unclaimed drops expire in 90 seconds.".into() });
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

    pub(super) fn update(&mut self, world: &World, dt: f32, resting: bool) -> Vec<Effect> {
        if dt <= 0.0 {
            return vec![];
        }
        let entered_rest = resting && !self.in_rest;
        self.in_rest = resting;
        if entered_rest {
            return self.begin_rest(world);
        }
        if let Some(remaining) = &mut self.remaining {
            *remaining -= dt;
            if *remaining <= 0.0 {
                self.remaining = None;
                self.pending_fill = [false; 2];
                let mut effects = Self::expire(world);
                effects.push(Effect::ShowMessage {
                    text: "Unclaimed supply drops expired.".into(),
                });
                return effects;
            }
        }
        let (tags, links) = world.borrow::<(View<PropEcoType>, View<Links>)>().unwrap();
        let carried = Self::carried(world);
        let mut effects = vec![];
        for floor in 0..2 {
            let cache = tags
                .iter()
                .with_id()
                .find(|(_, tag)| tag.0 == CACHE + floor as i32)
                .map(|(id, _)| id);
            if self.pending_fill[floor] {
                self.pending_fill[floor] = false;
                if let Some(container_entity_id) = cache {
                    let bonus = [-87, -938][self.roll(2)];
                    effects.extend([-1358, -52, -57, bonus].into_iter().map(|template_id| {
                        Effect::CreateEntityInContainer {
                            template_id,
                            container_entity_id,
                        }
                    }));
                }
            } else if let Some(entity_id) = cache {
                if !carried.contains(&entity_id)
                    && links.get(entity_id).is_ok_and(|links| {
                        !links
                            .to_links
                            .iter()
                            .any(|link| matches!(link.link, Link::Contains(_)))
                    })
                {
                    effects.push(Effect::DestroyEntity { entity_id });
                }
            }
        }
        effects
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dark::properties::{ToLink, WrappedEntityId};

    #[test]
    fn every_intermission_populates_both_floors_and_mixes_security_crates() {
        let world = World::new();
        let mut drops = SupplyDrops {
            seed: 42,
            ..Default::default()
        };
        let mut templates = HashSet::new();
        for _ in 0..20 {
            let effects = drops.update(&world, 1.0, true);
            let mut by_floor = [0, 0];
            for effect in effects {
                if let Effect::CreateEntity {
                    template_id,
                    position,
                    ..
                } = effect
                {
                    by_floor[usize::from(position.y > 10.0)] += 1;
                    templates.insert(template_id);
                }
            }
            assert_eq!(by_floor, [3, 3]);
            assert!(
                !drops
                    .update(&world, 1.0, true)
                    .iter()
                    .any(|e| matches!(e, Effect::CreateEntity { .. }))
            );
            drops.start_wave(&world, 1, false);
        }
        assert!(templates.contains(&-1886));
        assert!(templates.contains(&-941));
    }

    #[test]
    fn pending_fill_and_claim_clock_survive_save_without_duplicate_contents() {
        let mut world = World::new();
        let mut drops = SupplyDrops::default();
        drops.update(&world, 1.0, true);
        for floor in 0..2 {
            world.add_entity((PropEcoType(CACHE + floor), Links::empty()));
        }
        let mut loaded: SupplyDrops =
            serde_json::from_str(&serde_json::to_string(&drops).unwrap()).unwrap();
        assert_eq!(
            loaded
                .update(&world, 1.0, true)
                .iter()
                .filter(|e| matches!(e, Effect::CreateEntityInContainer { .. }))
                .count(),
            8
        );
        assert!(
            !loaded
                .update(&world, 1.0, true)
                .iter()
                .any(|e| matches!(e, Effect::CreateEntityInContainer { .. }))
        );
        assert_eq!(loaded.remaining, Some(88.0));
        assert!(loaded.update(&world, 0.0, true).is_empty());
        assert_eq!(loaded.remaining, Some(88.0));
    }

    #[test]
    fn expiry_removes_crate_contents_but_preserves_collected_items() {
        let mut world = World::new();
        let abandoned = world.add_entity((PropEcoType(PICKUP),));
        let collected = world.add_entity((PropEcoType(PICKUP),));
        let leftover = world.add_entity(());
        let contained = |id| Links {
            to_links: vec![ToLink {
                to_template_id: 0,
                to_entity_id: Some(WrappedEntityId(id)),
                link: Link::Contains(0),
            }],
        };
        let cache = world.add_entity((PropEcoType(CACHE), contained(leftover)));
        world.add_entity((contained(collected),));
        let mut drops = SupplyDrops {
            remaining: Some(1.0),
            in_rest: true,
            ..Default::default()
        };
        let effects = drops.update(&world, 2.0, false);
        for id in [abandoned, cache, leftover] {
            assert!(
                effects
                    .iter()
                    .any(|e| matches!(e, Effect::DestroyEntity { entity_id } if *entity_id == id))
            );
        }
        assert!(
            !effects.iter().any(
                |e| matches!(e, Effect::DestroyEntity { entity_id } if *entity_id == collected)
            )
        );
        assert!(drops.remaining.is_none());
    }
}
