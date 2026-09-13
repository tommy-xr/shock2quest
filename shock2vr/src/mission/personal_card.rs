//! Permanent personal credential/wallet tool. Ownership is transient presentation;
//! balances and permissions remain in QuestInfo. Reuses the body inventory frame.
use super::body_inventory::BodyPose;
use crate::{input_context::InputContext, vr_support::GripPose};
use cgmath::{InnerSpace, Matrix4, Quaternion, Rotation, Vector3, vec3};
use shipyard::{EntityId, Get, View, World};
use std::{cell::OnceCell, rc::Rc};

const DOWNLOAD_SECONDS: f32 = 1.15;

pub(crate) fn is_reader(world: &World, entity: EntityId) -> bool {
    world
        .borrow::<View<dark::properties::PropKeyDst>>()
        .is_ok_and(|keys| keys.get(entity).is_ok())
        || [
            "ReplicatorScript",
            "StatsTrainer",
            "WeaponTrainer",
            "TechTrainer",
            "PsiTrainer",
            "TraitMachine",
            "Computer",
            "SecurityComputer",
            "EnergyStation",
            "ResurrectMachine",
        ]
        .iter()
        .any(|script| crate::scripts::script_util::entity_has_script(world, entity, script))
}

pub(super) struct PersonalCard {
    pub center: Option<Vector3<f32>>,
    pub hand: Option<usize>,
    belt_yaw: f32,
    bit_textures: OnceCell<[Rc<dyn engine::texture::TextureTrait>; 2]>,
    pub blocked: [bool; 2],
    pressed: [bool; 2],
    tracked: [bool; 2],
    reader: Option<EntityId>,
    withdrawn: f32,
    pub scans: u64,
    pub last_scan: Option<EntityId>,
    pub grip: Option<crate::vr_grip::ResolvedGrip>,
    downloads: Vec<(Vector3<f32>, f32)>,
    pub held_pose: Option<(Vector3<f32>, Quaternion<f32>)>,
}

impl Default for PersonalCard {
    fn default() -> Self {
        Self {
            center: None,
            hand: None,
            belt_yaw: 0.0,
            bit_textures: OnceCell::new(),
            blocked: [false; 2],
            pressed: [true; 2],
            tracked: [false; 2],
            reader: None,
            withdrawn: 0.0,
            scans: 0,
            last_scan: None,
            held_pose: None,
            downloads: Vec::new(),
            grip: None,
        }
    }
}

impl PersonalCard {
    pub fn advance_downloads(&mut self, dt: f32) {
        for (_, age) in &mut self.downloads {
            *age += dt.max(0.0);
        }
        self.downloads.retain(|(_, age)| *age < DOWNLOAD_SECONDS);
    }

    pub fn download(&mut self, origin: Vector3<f32>) {
        self.downloads.push((origin, 0.0));
    }

    pub fn update(
        &mut self,
        input: &InputContext,
        body: Option<BodyPose>,
        available: [bool; 2],
        enabled: bool,
    ) {
        self.center = body.map(|body| {
            self.belt_yaw = body.yaw;
            // Match the existing astra-vr-belt.glb buckle node: x=-115 mm,
            // y=0, z=-316 mm relative to its front strap at z=-300 mm.
            // Another 8 mm clears the buckle's face. This is beside the pouch,
            // not at the belt origin (which is occupied by the ammo pouch).
            body.front(
                crate::dev_params::get(crate::dev_params::VR_BELT_DROP),
                crate::dev_params::get(crate::dev_params::VR_BELT_DISTANCE) + 0.024,
            ) - vec3(body.yaw.cos(), 0.0, body.yaw.sin()) * (0.115 / crate::METERS_PER_WORLD_UNIT)
        });
        self.blocked = [false; 2];
        if !enabled {
            self.reader = None;
            self.withdrawn = 0.0;
            self.hand = None;
            self.pressed = [true; 2];
            self.tracked = [false; 2];
            self.held_pose = None;
            return;
        }
        for (i, hand) in [&input.left_hand, &input.right_hand]
            .into_iter()
            .enumerate()
        {
            let tracked = body.is_some()
                && input.pose_tracking.is_none_or(|p| p.head && p.hands[i])
                && GripPose {
                    position: hand.position,
                    rotation: hand.rotation,
                }
                .is_tracked()
                && hand.squeeze_value.is_finite();
            let pressed = hand.squeeze_value >= 0.5;
            if self.hand == Some(i) {
                self.blocked[i] = true;
                // Tracking loss never invents a release. Recovery requires a
                // new squeeze before an open hand can return the card.
                if tracked && self.tracked[i] && !pressed && self.pressed[i] {
                    self.hand = None;
                    self.held_pose = None;
                    self.reader = None;
                    self.withdrawn = 0.0;
                }
            } else if self.hand.is_none()
                && available[i]
                && tracked
                && self.tracked[i]
                && pressed
                && !self.pressed[i]
                && self.center.is_some_and(|center| {
                    (hand.position - center).magnitude()
                        <= 0.05 / crate::METERS_PER_WORLD_UNIT
                            + super::body_inventory::hand_radius()
                })
            {
                self.hand = Some(i);
                self.blocked[i] = true;
            }
            self.pressed[i] = !tracked || pressed;
            self.tracked[i] = tracked;
        }
    }

    pub fn sample_hand(
        &mut self,
        input: &InputContext,
        pawn: Vector3<f32>,
        rotation: Quaternion<f32>,
    ) {
        if let Some(i) = self.hand.filter(|i| self.tracked[*i]) {
            let hand = [&input.left_hand, &input.right_hand][i];
            self.held_pose = Some((
                crate::virtual_hand::hand_world_position(pawn, rotation, hand.position),
                rotation * hand.rotation,
            ));
        }
    }

    /// A brief miss tolerates hand jitter. A different reader still requires
    /// withdrawing: brushing across two readers cannot issue two scans.
    pub fn scan(&mut self, target: Option<EntityId>, dt: f32) -> Option<EntityId> {
        if self.hand.is_none() || self.hand.is_some_and(|i| !self.tracked[i]) {
            return None;
        }
        if target.is_none() {
            self.withdrawn += dt.max(0.0);
            if self.withdrawn >= 0.25 {
                self.reader = None;
            }
            return None;
        }
        self.withdrawn = 0.0;
        if self.reader.is_some() {
            return None;
        }
        self.reader = target;
        self.last_scan = target;
        self.scans += 1;
        target
    }

    pub fn transform(&self, pawn: Vector3<f32>, rotation: Quaternion<f32>) -> Option<Matrix4<f32>> {
        if self.hand.is_some() {
            self.held_pose.map(|(position, orientation)| {
                let grip = self.grip.as_ref();
                Matrix4::from_translation(position)
                    * Matrix4::from(orientation)
                    * grip
                        .map(|grip| {
                            Matrix4::from_translation(grip.offset) * Matrix4::from(grip.rotation)
                        })
                        .unwrap_or_else(|| Matrix4::from_translation(vec3(0.0, 0.0, -0.1)))
            })
        } else {
            self.center.map(|center| {
                Matrix4::from_translation(pawn + rotation.rotate_vector(center))
                    * Matrix4::from(rotation)
                    * Matrix4::from_angle_y(cgmath::Rad(-self.belt_yaw))
                    * Matrix4::from_angle_x(cgmath::Deg(90.0))
                    // scipass lies in X/Z; turn its long Z edge horizontal, face upright.
                    * Matrix4::from_angle_y(cgmath::Deg(90.0))
            })
        }
    }

    pub fn render(
        &self,
        assets: &mut engine::assets::asset_cache::AssetCache,
        pawn: Vector3<f32>,
        rotation: Quaternion<f32>,
    ) -> Vec<engine::scene::SceneObject> {
        let Some(transform) = self.transform(pawn, rotation) else {
            return vec![];
        };
        let Some(model) = assets
            .get_opt::<_, dark::model::Model, _>(&dark::importers::MODELS_IMPORTER, "scipass.bin")
        else {
            return vec![];
        };
        let Some(bounds) = model.bounding_box() else {
            return vec![];
        };
        let size = bounds.max - bounds.min;
        let scale =
            (0.085 / crate::METERS_PER_WORLD_UNIT) / size.x.max(size.y).max(size.z).max(0.001);
        let center = (bounds.min.to_vec() + bounds.max.to_vec()) * 0.5;
        let model_transform = if self.hand.is_some() {
            if let Some(grip) = &self.grip {
                let anchor = vec3(grip.anchor[0], grip.anchor[1], grip.anchor[2]);
                Matrix4::from_translation(anchor * (grip.item_scale - scale))
                    * Matrix4::from_scale(scale)
            } else {
                Matrix4::from_scale(scale) * Matrix4::from_translation(-center)
            }
        } else {
            Matrix4::from_scale(scale) * Matrix4::from_translation(-center)
        };
        let mut scene: Vec<_> = model
            .clone_scene_objects()
            .into_iter()
            .map(|mut object| {
                object.set_transform(transform * model_transform);
                object
            })
            .collect();
        // Camera-facing binary fragments spiral along the transfer direction.
        // Two trailing samples make motion readable without solid geometry;
        // texture glow and fade keep the stream airy at headset distances.
        if !self.downloads.is_empty() {
            let textures = self
                .bit_textures
                .get_or_init(|| [download_bit_texture(false), download_bit_texture(true)]);
            let destination = transform.w.truncate();
            for (origin, age) in &self.downloads {
                let direction = destination - *origin;
                let axis = if direction.magnitude2() > 0.00001 {
                    direction.normalize()
                } else {
                    Vector3::unit_y()
                };
                let reference = if axis.dot(Vector3::unit_y()).abs() < 0.9 {
                    Vector3::unit_y()
                } else {
                    Vector3::unit_x()
                };
                let u = axis.cross(reference).normalize();
                let v = axis.cross(u);
                for i in 0..24 {
                    for tail in 0..3 {
                        let t = (*age - i as f32 * 0.016 - tail as f32 * 0.018) / 0.72;
                        if !(0.0..1.0).contains(&t) {
                            continue;
                        }
                        let phase = i as f32 * 2.399_963 + t * std::f32::consts::TAU * 1.7;
                        let envelope = (std::f32::consts::PI * t).sin();
                        let radius = (0.055 + (i % 4) as f32 * 0.012)
                            / crate::METERS_PER_WORLD_UNIT
                            * envelope;
                        let point = *origin
                            + direction * (t * t * (3.0 - 2.0 * t))
                            + (u * phase.cos() + v * phase.sin()) * radius;
                        let opacity = (t * 12.0).min(1.0)
                            * ((1.0 - t) * 9.0).min(1.0)
                            * [0.95, 0.22, 0.08][tail];
                        let color = if i % 3 == 0 {
                            vec3(0.55, 1.0, 0.85)
                        } else {
                            vec3(0.12, 0.80, 1.0)
                        };
                        let material = engine::scene::BillboardMaterial::create(
                            textures[i % 2].clone(),
                            color,
                            0.65,
                            1.0 - opacity,
                            (0.035 + (i % 3) as f32 * 0.004) / crate::METERS_PER_WORLD_UNIT,
                        );
                        let mut bit = engine::scene::SceneObject::new(
                            material,
                            Box::new(engine::scene::quad::create()),
                        );
                        bit.set_transform(Matrix4::from_translation(point));
                        scene.push(bit);
                    }
                }
            }
        }
        crate::util::tag_render_source(&mut scene, crate::util::render_source::PLAYER_HANDS);
        scene
    }
}
use cgmath::EuclideanSpace;

/// A tiny glowing bitmap glyph, cached once per mission. RGBA sprites use the
/// existing per-eye billboard material, so fragments face each eye correctly.
fn download_bit_texture(one: bool) -> Rc<dyn engine::texture::TextureTrait> {
    const ZERO: [&str; 7] = [
        "01110", "10001", "10001", "10001", "10001", "10001", "01110",
    ];
    const ONE: [&str; 7] = [
        "00100", "01100", "00100", "00100", "00100", "00100", "01110",
    ];
    let glyph = if one { ONE } else { ZERO };
    let mut bytes = Vec::with_capacity(64 * 64 * 4);
    for y in 0..64 {
        for x in 0..64 {
            let mut distance = f32::INFINITY;
            for (row, line) in glyph.iter().enumerate() {
                for (col, cell) in line.bytes().enumerate() {
                    if cell != b'1' {
                        continue;
                    }
                    let dx = ((x as f32 - (20.0 + col as f32 * 6.0)).abs() - 2.4).max(0.0);
                    let dy = ((y as f32 - (50.0 - row as f32 * 6.0)).abs() - 2.4).max(0.0);
                    distance = distance.min(dx.hypot(dy));
                }
            }
            let core = (1.0 - distance / 1.2).clamp(0.0, 1.0);
            let glow = 0.26 * (-distance * distance / 15.0).exp();
            bytes.extend_from_slice(&[255, 255, 255, ((core + glow).min(1.0) * 255.0) as u8]);
        }
    }
    Rc::new(engine::texture::init_from_memory2(
        engine::texture_format::RawTextureData {
            bytes,
            width: 64,
            height: 64,
            format: engine::texture_format::PixelFormat::RGBA,
        },
        &engine::texture::TextureOptions {
            wrap: false,
            ..Default::default()
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (PersonalCard, InputContext, BodyPose) {
        let mut input = InputContext::default();
        input.head.rotation = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        input.left_hand.rotation = input.head.rotation;
        input.right_hand.rotation = input.head.rotation;
        let body = BodyPose {
            head: vec3(0.0, 1.6, 0.0),
            yaw: 0.0,
        };
        (PersonalCard::default(), input, body)
    }
    #[test]
    fn landscape_card_follows_body_heading_without_claiming_pouch_center() {
        use cgmath::Transform;
        let (mut card, mut input, mut body) = fixture();
        body.yaw = 1.1;
        card.update(&input, Some(body), [true; 2], true);
        let transform = card
            .transform(vec3(0.0, 0.0, 0.0), Quaternion::new(1.0, 0.0, 0.0, 0.0))
            .unwrap();
        let long_edge = transform.transform_vector(Vector3::unit_z());
        assert!(
            long_edge.y.abs() < 0.0001,
            "long card edge must lie horizontally"
        );
        let body_right = vec3(body.yaw.cos(), 0.0, body.yaw.sin());
        assert!(long_edge.dot(body_right).abs() > 0.999);
        input.left_hand.position = super::super::ammo_pouch::center_for(body);
        card.update(&input, Some(body), [true; 2], true);
        input.left_hand.squeeze_value = 1.0;
        card.update(&input, Some(body), [true; 2], true);
        assert_eq!(
            card.hand, None,
            "a grip at the ammo pouch must not take the buckle card"
        );
    }

    #[test]
    fn one_permanent_card_draws_on_fresh_grip_and_returns_anywhere() {
        let (mut card, mut input, body) = fixture();
        card.update(&input, Some(body), [true; 2], true);
        input.left_hand.position = card.center.unwrap();
        input.right_hand.position = card.center.unwrap();
        card.update(&input, Some(body), [true; 2], true);
        input.left_hand.squeeze_value = 1.0;
        input.right_hand.squeeze_value = 1.0;
        card.update(&input, Some(body), [true; 2], true);
        assert_eq!(card.hand, Some(0));
        input.left_hand.position = vec3(5.0, 2.0, 5.0);
        input.left_hand.squeeze_value = 0.0;
        card.update(&input, Some(body), [true; 2], true);
        assert_eq!(card.hand, None);
        assert!(card.blocked[0], "return edge is consumed");
        card.update(&input, Some(body), [true; 2], true);
        assert_eq!(
            card.hand, None,
            "other hand's held squeeze cannot steal returned card"
        );
    }
    #[test]
    fn reader_requires_withdrawal_and_tracking_loss_cannot_rearm() {
        let (mut card, input, body) = fixture();
        card.update(&input, Some(body), [true; 2], true);
        card.hand = Some(0);
        let mut world = World::new();
        let a = world.add_entity(());
        let b = world.add_entity(());
        assert_eq!(card.scan(Some(a), 0.01), Some(a));
        assert_eq!(card.scan(Some(a), 1.0), None);
        assert_eq!(card.scan(Some(b), 1.0), None);
        card.tracked[0] = false;
        card.scan(None, 10.0);
        card.tracked[0] = true;
        assert_eq!(card.scan(Some(a), 0.01), None);
        card.scan(None, 0.3);
        assert_eq!(card.scan(Some(b), 0.01), Some(b));
        assert_eq!(card.scans, 2);
    }
    #[test]
    fn recovery_does_not_draw_from_a_stale_squeeze() {
        let (mut card, mut input, body) = fixture();
        card.update(&input, Some(body), [true; 2], true);
        input.left_hand.position = card.center.unwrap();
        input.left_hand.squeeze_value = 1.0;
        card.update(&input, None, [true; 2], true);
        card.update(&input, Some(body), [true; 2], true);
        assert_eq!(card.hand, None);
        input.left_hand.squeeze_value = 0.0;
        card.update(&input, Some(body), [true; 2], true);
        input.left_hand.squeeze_value = 1.0;
        card.update(&input, Some(body), [true; 2], true);
        assert_eq!(card.hand, Some(0));
    }
}
