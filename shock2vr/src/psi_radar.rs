//! Motion sensitivity: short-lived world-space echoes, shared by flat and VR.
use crate::{mission::PlayerInfo, psi::ActivePsiPowers};
use cgmath::{InnerSpace, Vector3, vec3};
use dark::properties::{AITeam, PropAI, PropAITeam, PropHitPoints, PropPosition};
use engine::scene::{RenderLayer, SceneObject, SceneObjectDebugTag, SkinnedMaterial};
use serde::Serialize;
use shipyard::{
    EntityId, Get, IntoIter, IntoWithId, Unique, UniqueView, UniqueViewMut, View, World,
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

pub const POWER: i32 = -1134;
// Original shkradar.cpp's 80 Dark units, converted to the port's world units.
const RANGE: f32 = 80.0 / dark::SCALE_FACTOR;
const ECHO_SECS: f32 = 0.75;
const MIN_SPEED: f32 = 0.08;
const MAX_CONTACTS: usize = 32;

#[derive(Clone, Debug, Serialize)]
pub struct RadarContact {
    pub entity_id: u64,
    pub position: [f32; 3],
    pub distance: f32,
    pub strength: f32,
}

#[derive(Default)]
struct MotionSample {
    position: Option<Vector3<f32>>,
    remaining: f32,
}
impl MotionSample {
    fn step(&mut self, position: Vector3<f32>, dt: f32) -> f32 {
        // Debug input patches and paused frames must not consume motion.
        if dt <= 0.0 {
            return self.remaining / ECHO_SECS;
        }
        self.remaining = (self.remaining - dt).max(0.0);
        if let Some(previous) = self.position {
            let speed = (position - previous).magnitude() / dt;
            if (MIN_SPEED..=32.0).contains(&speed) {
                self.remaining = ECHO_SECS;
            } else if speed > 32.0 {
                self.remaining = 0.0;
            }
        }
        self.position = Some(position);
        self.remaining / ECHO_SECS
    }
}

#[derive(Default, Unique)]
pub struct Radar {
    samples: HashMap<EntityId, MotionSample>,
    pub contacts: Vec<RadarContact>,
}

pub fn update(world: &World, dt: f32) {
    let active = world
        .borrow::<UniqueView<ActivePsiPowers>>()
        .is_ok_and(|powers| powers.is_active(POWER));
    if !active {
        let mut radar = world.borrow::<UniqueViewMut<Radar>>().unwrap();
        radar.samples.clear();
        radar.contacts.clear();
        return;
    }
    if dt <= 0.0 {
        return;
    }
    let player = world.borrow::<UniqueView<PlayerInfo>>().unwrap();
    let (ais, positions, health, teams) = world
        .borrow::<(
            View<PropAI>,
            View<PropPosition>,
            View<PropHitPoints>,
            View<PropAITeam>,
        )>()
        .unwrap();
    let mut radar = world.borrow::<UniqueViewMut<Radar>>().unwrap();
    radar.contacts.clear();
    let mut candidates = HashSet::new();
    for (id, (_, position, hp)) in (&ais, &positions, &health).iter().with_id() {
        let distance = (position.position - player.pos).magnitude();
        if id == player.entity_id
            || hp.hit_points <= 0
            || distance > RANGE
            || teams
                .get(id)
                .is_ok_and(|team| matches!(team.0, AITeam::Good | AITeam::Neutral))
            || !crate::util::has_refs(world, id)
        {
            continue;
        }
        let strength = radar
            .samples
            .entry(id)
            .or_default()
            .step(position.position, dt);
        candidates.insert(id);
        if strength > 0.0 {
            // Filled below after the mutable sample borrow ends.
            radar.contacts.push(RadarContact {
                entity_id: id.inner(),
                position: position.position.into(),
                distance,
                strength,
            });
        }
    }
    radar.samples.retain(|id, _| candidates.contains(id));
    radar.contacts.sort_by(|a, b| {
        a.distance
            .total_cmp(&b.distance)
            .then(a.entity_id.cmp(&b.entity_id))
    });
    radar.contacts.truncate(MAX_CONTACTS);
}

pub fn silhouette(object: &SceneObject, strength: f32, id: EntityId) -> SceneObject {
    let color = vec3(0.1, 0.8, 1.0);
    let material = object.material.borrow();
    let replacement = if let Some(skinned) = material.as_any().downcast_ref::<SkinnedMaterial>() {
        skinned.silhouette(color)
    } else {
        engine::scene::color_material::create(color)
    };
    let mut echo = object.clone();
    echo.material = Rc::new(RefCell::new(replacement));
    echo.set_transparency(Some(1.0 - 0.75 * strength));
    echo.set_depth_write(false);
    // SceneOverlay has its own depth, so walls cannot hide the echo. SceneUi
    // follows it, preserving viewmodels and readable HUDs on both runtimes.
    echo.set_render_layer(RenderLayer::SceneOverlay);
    echo.set_debug_tag(Some(Rc::new(SceneObjectDebugTag {
        entity_id: Some(id.inner()),
        source: Some("psi_radar".into()),
        ..Default::default()
    })));
    echo
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn contacts_drop_dead_distant_friendly_and_inactive_targets() {
        let mut world = World::new();
        let player = world.add_entity(());
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            inventory_entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
        });
        world.add_unique(ActivePsiPowers(vec![crate::psi::ActivePsiPower {
            template_id: POWER,
            name: "Radar".into(),
            remaining_secs: 180.0,
        }]));
        world.add_unique(Radar::default());
        let enemy = world.add_entity((
            PropAI("Human".into()),
            PropPosition {
                position: vec3(1.0, 0.0, 0.0),
                cell: 0,
                rotation: cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0),
            },
            PropHitPoints { hit_points: 10 },
            PropAITeam(AITeam::Bad1),
        ));
        let move_to = |x| {
            (&mut world.borrow::<shipyard::ViewMut<PropPosition>>().unwrap())
                .get(enemy)
                .unwrap()
                .position
                .x = x;
        };
        update(&world, 0.1);
        assert!(
            world
                .borrow::<UniqueView<Radar>>()
                .unwrap()
                .contacts
                .is_empty()
        );
        move_to(1.1);
        update(&world, 0.1);
        assert_eq!(
            world.borrow::<UniqueView<Radar>>().unwrap().contacts.len(),
            1
        );
        (&mut world.borrow::<shipyard::ViewMut<PropHitPoints>>().unwrap())
            .get(enemy)
            .unwrap()
            .hit_points = 0;
        update(&world, 0.1);
        assert!(
            world
                .borrow::<UniqueView<Radar>>()
                .unwrap()
                .contacts
                .is_empty()
        );
        (&mut world.borrow::<shipyard::ViewMut<PropHitPoints>>().unwrap())
            .get(enemy)
            .unwrap()
            .hit_points = 10;
        move_to(RANGE + 1.0);
        update(&world, 0.1);
        assert!(
            world
                .borrow::<UniqueView<Radar>>()
                .unwrap()
                .samples
                .is_empty()
        );
        move_to(1.0);
        update(&world, 0.1);
        move_to(1.1);
        update(&world, 0.1);
        assert_eq!(
            world.borrow::<UniqueView<Radar>>().unwrap().contacts.len(),
            1
        );
        (&mut world.borrow::<shipyard::ViewMut<PropAITeam>>().unwrap())
            .get(enemy)
            .unwrap()
            .0 = AITeam::Good;
        update(&world, 0.1);
        assert!(
            world
                .borrow::<UniqueView<Radar>>()
                .unwrap()
                .contacts
                .is_empty()
        );
        world
            .borrow::<UniqueViewMut<ActivePsiPowers>>()
            .unwrap()
            .0
            .clear();
        update(&world, 0.0);
        assert!(
            world
                .borrow::<UniqueView<Radar>>()
                .unwrap()
                .samples
                .is_empty()
        );
    }

    #[test]
    fn motion_echo_ignores_idle_zero_time_and_teleports_then_fades() {
        let mut sample = MotionSample::default();
        let origin = vec3(0.0, 0.0, 0.0);
        assert_eq!(sample.step(origin, 0.1), 0.0);
        assert_eq!(sample.step(origin, 0.1), 0.0);
        let moved = vec3(0.1, 0.0, 0.0);
        assert_eq!(sample.step(moved, 0.0), 0.0);
        assert_eq!(sample.step(moved, 0.1), 1.0);
        assert!(sample.step(moved, 0.3) < 1.0);
        assert_eq!(sample.step(moved, 0.5), 0.0);
        assert_eq!(sample.step(vec3(0.2, 0.0, 0.0), 0.1), 1.0);
        assert_eq!(sample.step(vec3(100.0, 0.0, 0.0), 0.1), 0.0);
    }
}
