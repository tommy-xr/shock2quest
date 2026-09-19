//! An interruptible, non-attacking installer. It borrows prop health and the
//! authored Talon model, so no combat AI is attached to the builder itself.
use cgmath::{
    Deg, EuclideanSpace, InnerSpace, Matrix4, Point3, Quaternion, Rotation3, SquareMatrix, Vector3,
};
use dark::properties::{PropEcoType, PropHitPoints, PropPosition};
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, IntoIter, IntoWithId, UniqueView, View, World};

use super::{PlayerInfo, entity_creator::CreateEntityOptions};
use crate::{
    dev_params,
    physics::{InternalCollisionGroups, PhysicsWorld},
    scripts::Effect,
};

const BUILDER: i32 = 57_000;
const BOX: i32 = 57_010;
const TURRET: i32 = 57_020;
// Street-only, surveyed floor sites. Flying segments are additionally checked
// against world geometry; a blocked route never teleports through a wall.
const SITES: [[f32; 3]; 3] = [[-4.0, 20.0, 28.0], [25.0, 20.0, 28.0], [4.0, 19.8, 24.0]];

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
enum Phase {
    #[default]
    Idle,
    Moving(usize),
    Building {
        site: usize,
        progress: f32,
    },
    Deploying(usize),
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(super) struct TechGrowth {
    phase: Phase,
    last_spawn_wave: u32,
    builder_seen: bool,
    last_position: [f32; 3],
    next_site: usize,
    commissioned: Vec<usize>,
}

fn create(
    template_id: i32,
    position: Vector3<f32>,
    tag: i32,
    name: &str,
    model: Option<&str>,
    hp: Option<i32>,
) -> Effect {
    Effect::CreateEntity {
        template_id,
        position: Point3::from_vec(position),
        orientation: Quaternion::from_angle_y(Deg(0.0)),
        root_transform: Matrix4::identity(),
        options: CreateEntityOptions {
            ecology_type: Some(tag),
            name_override: Some(name.into()),
            model_override: model.map(str::to_owned),
            hit_points_override: hp,
            ..Default::default()
        },
    }
}
fn at(site: usize, height: f32) -> Vector3<f32> {
    let mut position = Vector3::from(SITES[site]);
    position.y += height;
    position
}
fn clear_route(
    physics: &PhysicsWorld,
    builder: EntityId,
    from: Vector3<f32>,
    to: Vector3<f32>,
) -> bool {
    let delta = to - from;
    delta.magnitude2() < 0.01
        || physics
            .ray_cast2(
                Point3::from_vec(from),
                delta.normalize(),
                delta.magnitude(),
                InternalCollisionGroups::WORLD,
                Some(builder),
                true,
            )
            .is_none()
}

impl TechGrowth {
    pub(super) fn update(
        &mut self,
        world: &World,
        physics: &PhysicsWorld,
        dt: f32,
        active: bool,
        wave: u32,
        quick: bool,
    ) -> Vec<Effect> {
        let (tags, positions, hp) = world
            .borrow::<(View<PropEcoType>, View<PropPosition>, View<PropHitPoints>)>()
            .unwrap();
        let actors: Vec<_> = (&tags, &positions, &hp)
            .iter()
            .with_id()
            .filter(|(_, (tag, _, hp))| {
                (BUILDER..TURRET + SITES.len() as i32).contains(&tag.0) && hp.hit_points > 0
            })
            .map(|(id, (tag, pos, _))| (tag.0, id, pos.position))
            .collect();
        let find = |tag| actors.iter().find(|(kind, _, _)| *kind == tag).copied();
        let mut effects = vec![];
        // A junction is the local power source. Destroying it removes its
        // turret even during rest or while the developer toggle is disabled.
        self.commissioned.retain(|&site| {
            if find(BOX + site as i32).is_some() {
                return true;
            }
            if let Some((_, entity_id, _)) = find(TURRET + site as i32) {
                effects.push(Effect::DestroyEntity { entity_id });
            }
            false
        });
        let builder = find(BUILDER);
        if builder.is_none() && self.builder_seen {
            self.builder_seen = false;
            self.last_spawn_wave = wave;
            if let Phase::Building { site, .. } | Phase::Deploying(site) = self.phase {
                for tag in [BOX + site as i32, TURRET + site as i32] {
                    if let Some((_, entity_id, _)) = find(tag) {
                        effects.push(Effect::DestroyEntity { entity_id });
                    }
                }
            }
            self.phase = Phase::Idle;
            effects.push(create(
                -87,
                self.last_position.into(),
                0,
                "Installer salvage",
                None,
                None,
            ));
            effects.push(Effect::ShowMessage { text: "Installer destroyed. Construction stopped for this wave; completed turrets remain.".into() });
        }
        if let Some((_, _, position)) = builder {
            self.builder_seen = true;
            self.last_position = position.into();
        }
        if !active
            || !dev_params::get_bool(dev_params::HORDE_TECH_ENABLED)
            || (!quick && wave < dev_params::get(dev_params::HORDE_TECH_WAVE) as u32)
        {
            return effects;
        }
        let Some((_, builder, position)) = builder else {
            if wave == self.last_spawn_wave {
                return effects;
            }
            // Keep the first encounter in the street arena where it can be seen.
            let Ok(player) = world.borrow::<UniqueView<PlayerInfo>>() else {
                return effects;
            };
            if player.pos.y < 10.0 {
                return effects;
            }
            let site = (0..SITES.len())
                .max_by(|&a, &b| {
                    (at(a, 2.4) - player.pos)
                        .magnitude2()
                        .total_cmp(&(at(b, 2.4) - player.pos).magnitude2())
                })
                .unwrap();
            self.last_spawn_wave = wave;
            self.phase = Phase::Idle;
            self.last_position = at(site, 2.4).into();
            effects.push(create(
                -760,
                at(site, 2.4),
                BUILDER,
                "Talon installer",
                Some("talond"),
                Some(20),
            ));
            effects.push(Effect::ShowMessage {
                text: "Talon installer on the street. Stop it before it builds turrets.".into(),
            });
            return effects;
        };
        self.builder_seen = true;
        self.last_position = position.into();
        let work_dt = dt * if quick { 5.0 } else { 1.0 };
        match self.phase.clone() {
            Phase::Idle => {
                for offset in 0..SITES.len() {
                    let site = (self.next_site + offset) % SITES.len();
                    if find(BOX + site as i32).is_none()
                        && find(TURRET + site as i32).is_none()
                        && clear_route(physics, builder, position, at(site, 2.4))
                    {
                        self.next_site = (site + 1) % SITES.len();
                        self.phase = Phase::Moving(site);
                        break;
                    }
                }
            }
            Phase::Moving(site) => {
                let target = at(site, 2.4);
                if !clear_route(physics, builder, position, target) {
                    self.phase = Phase::Idle;
                } else {
                    let delta = target - position;
                    let distance = delta.magnitude();
                    if distance > 0.1 {
                        let step = (dt * 2.0).min(distance);
                        effects.push(Effect::SetPosition {
                            entity_id: builder,
                            position: position + delta / distance * step,
                        });
                    } else {
                        effects.push(create(
                            -760,
                            at(site, 0.4),
                            BOX + site as i32,
                            "Turret junction — destroy to disable",
                            None,
                            Some(12),
                        ));
                        self.phase = Phase::Building {
                            site,
                            progress: 0.0,
                        };
                        effects.push(Effect::ShowMessage { text: "Installer constructing a turret. Destroy the installer or its junction box.".into() });
                    }
                }
            }
            Phase::Building { site, progress } => {
                if let Some((_, junction, _)) = find(BOX + site as i32) {
                    let progress = (progress
                        + work_dt / dev_params::get(dev_params::HORDE_TECH_BUILD_SECONDS))
                    .min(1.0);
                    effects.push(Effect::SetRenderAlpha {
                        entity_id: junction,
                        alpha: 0.35 + progress * 0.65,
                    });
                    effects.push(Effect::SetRotation {
                        entity_id: builder,
                        rotation: Quaternion::from_angle_y(Deg(progress * 720.0)),
                    });
                    if progress >= 1.0 {
                        effects.push(create(
                            -369,
                            at(site, 1.3),
                            TURRET + site as i32,
                            "Installed slug turret",
                            None,
                            None,
                        ));
                        self.phase = Phase::Deploying(site);
                    } else {
                        self.phase = Phase::Building { site, progress };
                    }
                } else {
                    self.phase = Phase::Idle;
                }
            }
            Phase::Deploying(site) => {
                if find(BOX + site as i32).is_none() {
                    if let Some((_, entity_id, _)) = find(TURRET + site as i32) {
                        effects.push(Effect::DestroyEntity { entity_id });
                    }
                    self.phase = Phase::Idle;
                } else if find(TURRET + site as i32).is_some() {
                    self.commissioned.push(site);
                    self.phase = Phase::Idle;
                    effects.push(Effect::ShowMessage {
                        text: "Turret online. Its junction box is the weak point.".into(),
                    });
                } else {
                    // Creation was not observed: keep the job pending and retry.
                    self.phase = Phase::Building {
                        site,
                        progress: 0.99,
                    };
                }
            }
        }
        effects
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn actor(world: &mut World, tag: i32, site: usize) -> EntityId {
        world.add_entity((
            PropEcoType(tag),
            PropHitPoints { hit_points: 20 },
            PropPosition {
                position: at(site, 2.4),
                cell: u16::MAX,
                rotation: Quaternion::from_angle_y(Deg(0.0)),
            },
        ))
    }
    fn world() -> World {
        let mut world = World::new();
        let player = world.add_entity(());
        world.add_unique(PlayerInfo {
            entity_id: player,
            inventory_entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            pos: at(0, 1.0),
            rotation: Quaternion::from_angle_y(Deg(0.0)),
        });
        world
    }
    #[test]
    fn installer_requires_combat_and_wave_unlock_and_has_no_combat_template() {
        let world = world();
        let physics = PhysicsWorld::new();
        let mut tech = TechGrowth::default();
        assert!(
            tech.update(&world, &physics, 100.0, false, 5, false)
                .is_empty()
        );
        assert!(
            tech.update(&world, &physics, 100.0, true, 4, false)
                .is_empty()
        );
        assert!(tech.update(&world, &physics, 1.0, true, 5, false).iter().any(|effect| matches!(effect,
            Effect::CreateEntity { template_id: -760, options, .. } if options.model_override.as_deref() == Some("talond"))));
        assert!(
            tech.update(&world, &physics, 1.0, true, 5, false)
                .is_empty()
        );
    }
    #[test]
    fn construction_pauses_and_resumes_across_save_then_commissions_turret() {
        let mut world = world();
        let physics = PhysicsWorld::new();
        actor(&mut world, BUILDER, 0);
        actor(&mut world, BOX, 0);
        let mut tech = TechGrowth {
            phase: Phase::Building {
                site: 0,
                progress: 0.5,
            },
            ..Default::default()
        };
        assert!(
            tech.update(&world, &physics, 100.0, false, 5, false)
                .is_empty()
        );
        let mut loaded: TechGrowth =
            serde_json::from_str(&serde_json::to_string(&tech).unwrap()).unwrap();
        let effects = loaded.update(&world, &physics, 12.5, true, 5, false);
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::CreateEntity {
                template_id: -369,
                ..
            }
        )));
        actor(&mut world, TURRET, 0);
        loaded.update(&world, &physics, 1.0, true, 5, false);
        assert_eq!(loaded.commissioned, vec![0]);
    }
    #[test]
    fn destroying_junction_removes_its_turret_even_during_rest() {
        let mut world = world();
        let physics = PhysicsWorld::new();
        let turret = actor(&mut world, TURRET, 0);
        let mut tech = TechGrowth {
            commissioned: vec![0],
            ..Default::default()
        };
        assert!(
            tech.update(&world, &physics, 1.0, false, 5, false)
                .iter()
                .any(|effect| matches!(effect,
            Effect::DestroyEntity { entity_id } if *entity_id == turret))
        );
        assert!(tech.commissioned.is_empty());
    }
    #[test]
    fn full_sites_prevent_additional_construction() {
        let mut world = world();
        let physics = PhysicsWorld::new();
        actor(&mut world, BUILDER, 0);
        for site in 0..SITES.len() {
            actor(&mut world, BOX + site as i32, site);
            actor(&mut world, TURRET + site as i32, site);
        }
        let mut tech = TechGrowth {
            commissioned: vec![0, 1, 2],
            ..Default::default()
        };
        for _ in 0..10 {
            assert!(
                tech.update(&world, &physics, 100.0, true, 5, false)
                    .is_empty()
            );
        }
    }

    #[test]
    fn killing_installer_cancels_unfinished_job_and_drops_salvage_only_once() {
        let mut world = world();
        let physics = PhysicsWorld::new();
        let junction = actor(&mut world, BOX, 0);
        let mut tech = TechGrowth {
            builder_seen: true,
            last_spawn_wave: 5,
            phase: Phase::Building {
                site: 0,
                progress: 0.9,
            },
            ..Default::default()
        };
        let effects = tech.update(&world, &physics, 1.0, true, 6, false);
        assert!(effects.iter().any(
            |effect| matches!(effect, Effect::DestroyEntity { entity_id } if *entity_id == junction)
        ));
        assert_eq!(
            effects
                .iter()
                .filter(|effect| matches!(
                    effect,
                    Effect::CreateEntity {
                        template_id: -87,
                        ..
                    }
                ))
                .count(),
            1
        );
        assert!(
            tech.update(&world, &physics, 1.0, true, 6, false)
                .is_empty()
        );
    }
}
