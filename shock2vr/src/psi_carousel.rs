//! Lightweight amp-local browsing. Navigation previews; closing commits once.
use crate::{
    Handedness,
    psi::{GlobalPsiPowers, PlayerPsiKnownPowers},
    ui::{HAlign, Rect, UiCanvas, VAlign},
};
use cgmath::vec2;
use shipyard::{EntityId, UniqueView, World};

/// Reject button gestures that started before a selector opened/closed.
#[derive(shipyard::Component, Default)]
pub(crate) struct InputEpoch(pub u64);
pub(crate) fn input_epoch(world: &World, amp: EntityId) -> u64 {
    use shipyard::Get;
    world
        .borrow::<shipyard::View<InputEpoch>>()
        .ok()
        .and_then(|v| v.get(amp).ok().map(|e| e.0))
        .unwrap_or(0)
}
pub(crate) fn advance_input_epoch(world: &mut World, amp: EntityId) {
    let next = input_epoch(world, amp).wrapping_add(1);
    world.add_component(amp, InputEpoch(next));
}

pub(crate) struct Carousel {
    pub amp: EntityId,
    pub hand: Handedness,
    pub index: usize,
    pub stick_latched: bool,
    pub trigger_armed: bool,
    previous_index: usize,
    starts: Vec<(usize, f32, f32)>,
    transition: f32,
}

/// A sparse set of purchased powers on the selected tier and its trained
/// neighbours. Empty tiers and duplicate rows never appear.
fn entries(
    powers: &[crate::psi::PsiPowerInfo],
    known: &std::collections::HashSet<i32>,
    selected: usize,
) -> Vec<(usize, i32, i32)> {
    let mut tiers: Vec<_> = powers
        .iter()
        .filter(|p| known.contains(&p.template_id))
        .map(|p| p.tier())
        .collect();
    tiers.sort_unstable();
    tiers.dedup();
    let Some(tier) = powers.get(selected).map(|p| p.tier()) else {
        return vec![];
    };
    let Some(center) = tiers.iter().position(|t| *t == tier) else {
        return vec![];
    };
    let rows: &[i32] = match tiers.len() {
        0 => &[],
        1 => &[0],
        2 => &[0, 1],
        _ => &[-1, 0, 1],
    };
    let mut result = Vec::new();
    for &row in rows {
        let t = tiers[(center as i32 + row).rem_euclid(tiers.len() as i32) as usize];
        let purchased: Vec<_> = powers
            .iter()
            .enumerate()
            .filter(|(_, p)| p.tier() == t && known.contains(&p.template_id))
            .map(|(i, _)| i)
            .collect();
        let middle = purchased.iter().position(|i| *i == selected).unwrap_or(0) as i32;
        let n = purchased.len() as i32;
        for (column, index) in purchased.into_iter().enumerate() {
            let slot = (column as i32 - middle + n / 2).rem_euclid(n) - n / 2;
            result.push((index, row, slot));
        }
    }
    result
}

/// Shared spherical layout in world units. Both presentations render these
/// same cards; there is no separate flat placement or text-sizing path.
fn arc_position(row: f32, slot: f32) -> cgmath::Vector3<f32> {
    let yaw = slot * 0.23;
    let pitch = row * 0.25;
    cgmath::vec3(
        0.85 * yaw.sin() * pitch.cos(),
        -0.85 * pitch.sin(),
        0.85 * (yaw.cos() * pitch.cos() - 1.0),
    )
}

/// One shared front-facing arc for every latitude, including sparse tiers.
/// Bounding the common rotation prevents a singleton row drifting behind the amp.
fn visible_slot(slot: f32) -> f32 {
    (slot + 3.5).rem_euclid(7.0) - 3.5
}

fn projection_frame(
    origin: cgmath::Vector3<f32>,
    eye: cgmath::Vector3<f32>,
) -> cgmath::Matrix4<f32> {
    use cgmath::{InnerSpace, Matrix4, vec3};
    let direction = eye - origin;
    if !direction.magnitude2().is_finite() || direction.magnitude2() < 1e-6 {
        return Matrix4::from_translation(origin);
    }
    let normal = direction.normalize();
    let right = vec3(0.0, 1.0, 0.0).cross(normal);
    if right.magnitude2() < 1e-6 {
        return Matrix4::from_translation(origin);
    }
    let right = right.normalize();
    Matrix4::from_cols(
        right.extend(0.0),
        normal.cross(right).extend(0.0),
        normal.extend(0.0),
        origin.extend(1.0),
    )
}

fn luminous_icon(canvas: &mut UiCanvas, rect: Rect, texture: &str, alpha: f32) {
    canvas.push(crate::ui::UiElement::Image {
        position: vec2(rect.x, rect.y),
        size: vec2(rect.w, rect.h),
        texture: texture.into(),
        alpha,
        kind: crate::ui::ImageKind::HolographicIcon,
    });
}

impl Carousel {
    pub fn new(world: &World, amp: EntityId, hand: Handedness) -> Option<Self> {
        let selected = crate::psi_amp_selection::selection(world, amp)?;
        let powers = world.borrow::<UniqueView<GlobalPsiPowers>>().ok()?;
        let index = powers
            .0
            .iter()
            .position(|p| p.template_id == selected.current)?;
        // An amp can predate the player's first purchase. Preview a known
        // power without rewriting its saved pair until the user confirms.
        let known = world.borrow::<UniqueView<PlayerPsiKnownPowers>>().ok()?;
        let index = if known.0.contains(&powers.0[index].template_id) {
            index
        } else {
            powers
                .0
                .iter()
                .position(|p| known.0.contains(&p.template_id))
                .unwrap_or(index)
        };
        Some(Self {
            amp,
            hand,
            index,
            previous_index: index,
            starts: entries(&powers.0, &known.0, index)
                .into_iter()
                .map(|(i, r, s)| (i, r as f32, s as f32))
                .collect(),
            transition: 1.0,
            stick_latched: true,
            trigger_armed: false,
        })
    }
    pub fn preview(
        &mut self,
        index: usize,
        powers: &[crate::psi::PsiPowerInfo],
        known: &std::collections::HashSet<i32>,
    ) {
        if self.index != index {
            self.starts = self
                .positions(powers, known)
                .into_iter()
                .map(|(i, _, _, r, s)| (i, r, visible_slot(s)))
                .collect();
            self.previous_index = self.index;
            self.index = index;
            self.transition = 0.0;
        }
    }
    pub fn update(&mut self, dt: f32) {
        self.transition = (self.transition + dt / 0.25).min(1.0);
    }

    /// Interpolate angles, not a chord between positions. Snapshotting these
    /// angles on another nudge makes interrupted transitions continuous.
    fn positions(
        &self,
        powers: &[crate::psi::PsiPowerInfo],
        known: &std::collections::HashSet<i32>,
    ) -> Vec<(usize, i32, i32, f32, f32)> {
        let eased = self.transition * self.transition * (3.0 - 2.0 * self.transition);
        let same_tier = powers[self.previous_index].tier() == powers[self.index].tier();
        entries(powers, known, self.index)
            .into_iter()
            .map(|(index, row, slot)| {
                let start = self
                    .starts
                    .iter()
                    .find(|(i, _, _)| *i == index)
                    .map(|(_, r, s)| (*r, *s))
                    .unwrap_or((row as f32, slot as f32));
                let slot_position = if same_tier {
                    let shift = self
                        .starts
                        .iter()
                        .find(|(i, _, _)| *i == self.index)
                        .map(|(_, _, s)| *s)
                        .unwrap_or(0.0);
                    let shift = visible_slot(shift);
                    start.1 - shift * eased
                } else {
                    start.1 + (slot as f32 - start.1) * eased
                };
                (
                    index,
                    row,
                    slot,
                    start.0 + (row as f32 - start.0) * eased,
                    slot_position,
                )
            })
            .collect()
    }

    pub fn render(
        &self,
        world: &World,
        assets: &mut engine::assets::asset_cache::AssetCache,
        eye: cgmath::Vector3<f32>,
    ) -> Vec<engine::scene::SceneObject> {
        use cgmath::{Matrix4, Rad, vec3};
        let Some(amp_frame) = crate::psi_sword::frame(world, self.amp) else {
            return vec![];
        };
        // Attach to the synchronized physical amp, not a raw controller pose.
        // The projection stays upright and faces the viewer as the amp moves.
        let root = projection_frame(amp_frame.w.truncate() + vec3(0.0, 0.49, 0.0), eye)
            * Matrix4::from_scale(0.5);
        let Ok(powers) = world.borrow::<UniqueView<GlobalPsiPowers>>() else {
            return vec![];
        };
        let Ok(known) = world.borrow::<UniqueView<PlayerPsiKnownPowers>>() else {
            return vec![];
        };
        let visible = self.positions(&powers.0, &known.0);
        let strings = world
            .borrow::<UniqueView<crate::scripts::gui::GlobalPsiStrings>>()
            .ok();
        let empty = std::collections::HashMap::new();
        let strings = strings.as_ref().map_or(&empty, |s| &s.0);
        let icon = |id| {
            crate::scripts::gui::icon_texture(&crate::scripts::gui::icon_basename(strings, id), 1)
        };
        let pair = crate::psi_amp_selection::selection(world, self.amp);
        let mut objects = Vec::new();
        let mut drawn_tiers = std::collections::HashSet::new();
        for &(index, row, slot, row_position, slot_position) in &visible {
            let p = &powers.0[index];
            let slot_position = visible_slot(slot_position);
            let end = arc_position(row as f32, slot as f32);
            let focus = (1.0 - (row_position.abs() + slot_position.abs()) * 1.5).clamp(0.0, 1.0);
            let position =
                arc_position(row_position, slot_position) + vec3(0.0, 0.0, focus * 0.045);
            let size = 0.145 + 0.035 * focus;
            let edge = ((3.5 - slot_position.abs()) * 2.0).clamp(0.0, 1.0);
            let alpha = (if row == 0 { 0.72 } else { 0.42 } + 0.28 * focus) * edge;
            let mut card = UiCanvas::new(vec2(48.0, 48.0));
            luminous_icon(
                &mut card,
                Rect::new(3.0, 3.0, 42.0, 42.0),
                &icon(p.power.power_id),
                alpha,
            );
            if let Some(label) = pair.and_then(|pair| {
                if pair.current == p.template_id {
                    Some("C")
                } else if pair.alternate == Some(p.template_id) {
                    Some("A")
                } else {
                    None
                }
            }) {
                // Same framed corner badge as inventory L/R, attached to the
                // actual power icon instead of duplicated in a separate legend.
                let badge = Rect::new(33.0, 35.0, 15.0, 12.0);
                card.image(badge, "frame.pcx").opacity(edge);
                card.text(
                    badge,
                    label,
                    crate::ui::MFD_FONT,
                    11.0,
                    HAlign::Center,
                    VAlign::Middle,
                )
                .opacity(edge);
            }
            let transform = root
                * Matrix4::from_translation(position)
                * Matrix4::from_angle_y(Rad(slot_position * 0.13))
                * Matrix4::from_angle_x(Rad(row_position * 0.12))
                * Matrix4::from_scale(size);
            objects.extend(card.render_world_space(assets, transform, None, None, 0.001));
            if drawn_tiers.insert(p.tier()) {
                let mut label = UiCanvas::new(vec2(72.0, 20.0));
                label
                    .text(
                        Rect::new(0.0, 0.0, 72.0, 20.0),
                        &format!("{}", p.tier()),
                        crate::ui::MFD_FONT,
                        14.0,
                        HAlign::Center,
                        VAlign::Middle,
                    )
                    .opacity(if row == 0 { 0.9 } else { 0.4 });
                objects.extend(label.render_world_space(
                    assets,
                    root * Matrix4::from_translation(vec3(-0.68, end.y, -0.04))
                        * Matrix4::from_nonuniform_scale(0.20, 0.20 * 20.0 / 72.0, 1.0),
                    None,
                    None,
                    0.001,
                ));
            }
        }
        // This focal mark never follows an icon. Incoming powers brighten and
        // lift as their interpolated spherical position approaches it.
        let mut focus = UiCanvas::new(vec2(48.0, 48.0));
        focus.fill(Rect::new(14.0, 47.0, 20.0, 1.0), [90, 226, 255]);
        objects.extend(focus.render_world_space(
            assets,
            root * Matrix4::from_translation(vec3(0.0, 0.0, 0.045)) * Matrix4::from_scale(0.18),
            None,
            None,
            0.001,
        ));
        let mut details = UiCanvas::new(vec2(440.0, 26.0));
        if let Some(p) = powers
            .0
            .get(self.index)
            .filter(|p| known.0.contains(&p.template_id))
        {
            let title = p.display_name.as_deref().unwrap_or(&p.name);
            details.text_fit(
                Rect::new(0.0, 0.0, 440.0, 24.0),
                &format!("{title}  ·  {} PSI", p.power.psi_cost),
                crate::ui::MFD_FONT,
                19.0,
                HAlign::Center,
                VAlign::Middle,
            );
        } else {
            details.text(
                Rect::new(0.0, 0.0, 440.0, 24.0),
                "NO TRAINED POWERS",
                crate::ui::MFD_FONT,
                18.0,
                HAlign::Center,
                VAlign::Middle,
            );
        }
        objects.extend(details.render_world_space(
            assets,
            root * Matrix4::from_translation(vec3(0.0, -0.33, 0.025))
                * Matrix4::from_nonuniform_scale(1.10, 1.10 * 26.0 / 440.0, 1.0),
            None,
            None,
            0.001,
        ));
        for object in &mut objects {
            object.set_depth_write(false);
        }
        objects
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{InnerSpace, vec3};
    use std::collections::HashSet;

    fn powers() -> Vec<crate::psi::PsiPowerInfo> {
        [1, 2, 9, 17, 18, 33]
            .into_iter()
            .map(|id| crate::psi::PsiPowerInfo {
                template_id: -id,
                name: String::new(),
                display_name: None,
                power: dark::properties::PropPsiPower {
                    power_id: id,
                    activation_type: 0,
                    psi_cost: 1,
                    data: [0.0; 4],
                },
                projectiles: vec![],
                overloadable: false,
                duration: None,
            })
            .collect()
    }

    #[test]
    fn projection_only_contains_purchased_powers_and_skips_empty_tiers() {
        let powers = powers();
        let known = HashSet::from([-1, -17, -18, -33]);
        let visible = entries(&powers, &known, 4);
        assert_eq!(
            visible
                .iter()
                .map(|(i, _, _)| powers[*i].template_id)
                .collect::<HashSet<_>>(),
            known
        );
        assert!(
            visible.contains(&(4, 0, 0)),
            "previewed icon occupies the front center"
        );
        assert!(visible.contains(&(0, -1, 0)));
        assert!(visible.contains(&(5, 1, 0)));
        assert_eq!(entries(&powers, &HashSet::from([-17]), 3), vec![(3, 0, 0)]);
        let two = entries(&powers, &HashSet::from([-1, -33]), 0);
        assert_eq!(two.len(), 2, "two purchased tiers must not duplicate a row");
        assert!(entries(&powers, &HashSet::new(), 0).is_empty());
    }

    #[test]
    fn first_purchase_after_holding_an_amp_is_visible_without_committing_it() {
        let mut world = World::new();
        let amp = world.add_entity(());
        world.add_unique(GlobalPsiPowers(powers()));
        world.add_unique(PlayerPsiKnownPowers(HashSet::new()));
        crate::psi_amp_selection::initialize(&mut world, amp);
        let original = crate::psi_amp_selection::selection(&world, amp);
        world
            .borrow::<shipyard::UniqueViewMut<PlayerPsiKnownPowers>>()
            .unwrap()
            .0
            .insert(-17);
        let menu = Carousel::new(&world, amp, Handedness::Right).unwrap();
        assert_eq!(menu.index, 3);
        assert_eq!(crate::psi_amp_selection::selection(&world, amp), original);
    }

    #[test]
    fn reversing_mid_turn_continues_from_the_interpolated_sphere_position() {
        let powers = powers();
        let known = HashSet::from([-1, -2]);
        let mut world = World::new();
        let amp = world.add_entity(());
        world.add_unique(GlobalPsiPowers(powers.clone()));
        world.add_unique(PlayerPsiKnownPowers(known.clone()));
        let mut menu = Carousel::new(&world, amp, Handedness::Right).unwrap();
        menu.preview(1, &powers, &known);
        menu.update(0.125);
        let before = menu.positions(&powers, &known);
        menu.preview(0, &powers, &known);
        let after = menu.positions(&powers, &known);
        let a = before.iter().find(|(i, _, _, _, _)| *i == 0).unwrap();
        let b = after.iter().find(|(i, _, _, _, _)| *i == 0).unwrap();
        assert_eq!((a.3, a.4), (b.3, b.4));
        menu.update(0.25);
        let settled = menu.positions(&powers, &known);
        let focus = settled.iter().find(|(i, _, _, _, _)| *i == 0).unwrap();
        assert_eq!((focus.3, focus.4), (0.0, 0.0));
    }

    #[test]
    fn a_full_rotation_keeps_sparse_neighbouring_tiers_on_the_visible_arc() {
        let fixture = powers();
        let mut powers: Vec<_> = (1..=7)
            .map(|id| {
                let mut p = fixture[0].clone();
                p.template_id = -id;
                p.power.power_id = id;
                p
            })
            .collect();
        powers.push(fixture[3].clone());
        let known: HashSet<_> = powers.iter().map(|p| p.template_id).collect();
        let mut world = World::new();
        let amp = world.add_entity(());
        world.add_unique(GlobalPsiPowers(powers.clone()));
        world.add_unique(PlayerPsiKnownPowers(known.clone()));
        let mut menu = Carousel::new(&world, amp, Handedness::Right).unwrap();
        for i in 1..=28 {
            menu.preview(i % 7, &powers, &known);
            menu.update(0.25);
            let positions = menu.positions(&powers, &known);
            let sparse = positions.iter().find(|(i, _, _, _, _)| *i == 7).unwrap();
            assert!(visible_slot(sparse.4).abs() <= 3.5);
            if i % 7 == 0 {
                assert!(
                    visible_slot(sparse.4).abs() < 1e-5,
                    "a full cycle returns the sparse neighbour"
                );
            }
        }
    }

    #[test]
    fn curved_cards_recede_symmetrically_and_mount_follows_the_amp() {
        let left = arc_position(0.0, -2.0);
        let right = arc_position(0.0, 2.0);
        assert!((left.x + right.x).abs() < 1e-6);
        assert_eq!(left.z, right.z);
        assert!(left.z < arc_position(0.0, 0.0).z);
        let eye = vec3(0.0, 1.6, 0.0);
        for origin in [vec3(-0.4, 1.0, -0.7), vec3(0.4, 1.0, -0.7)] {
            let frame = projection_frame(origin, eye);
            assert_eq!(frame.w.truncate(), origin);
            assert!(frame.z.truncate().dot((eye - origin).normalize()) > 0.999);
            assert!(
                frame
                    .x
                    .truncate()
                    .cross(frame.y.truncate())
                    .dot(frame.z.truncate())
                    > 0.999
            );
        }
    }
}
