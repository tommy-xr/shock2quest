use cgmath::{
    Deg, EuclideanSpace, InnerSpace, Matrix4, Quaternion, Rotation, Rotation3, Transform, point3,
};
use dark::{
    SCALE_FACTOR,
    properties::{GunFlashOptions, GunSettingDesc, Link, ProjectileOptions},
};
use engine::audio::AudioHandle;
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{
    mission::{entity_creator::CreateEntityOptions, mission_core::GlobalTemplateClassTags},
    physics::{InternalCollisionGroups, PhysicsWorld, RayCastResult},
    runtime_props::{
        RuntimePropFlatAim, RuntimePropReloading, RuntimePropSelectedAmmo, RuntimePropShotCooldown,
        RuntimePropShotModifiers, RuntimePropTransform, RuntimePropVhots,
    },
    util::{get_rotation_from_forward_vector, resolve_proxy_entity},
    vr_config,
};

/// How far in front of the camera a flat-aimed projectile spawns, so a slow
/// physics projectile (e.g. a grenade) clears the player's own collider instead
/// of spawning inside it. World units.
const FLAT_MUZZLE_CLEARANCE: f32 = SCALE_FACTOR;

/// Reach of a flat melee swing (raycast along the crosshair ray), in world units.
const MELEE_RANGE: f32 = 1.2;
/// Damage dealt by a flat melee hit. TODO: derive from the weapon's `Melee Typ`.
const MELEE_DAMAGE: f32 = 6.0;

/// How far a gunshot carries to alert AIs (50 Dark feet). One value for all
/// guns for now; per-weapon loudness is a follow-up.
const GUNSHOT_NOISE_RADIUS: f32 = 50.0 / SCALE_FACTOR;

/// Rotation taking the projectile's +Z travel axis onto the barrel of a 25AE
/// first-person gun model, which is authored along the model's **-X** - that is
/// where each of those meshes puts its muzzle vhot, and pointing that axis out
/// of the hand is the whole job of every gun's -90 degree yaw in the
/// `vr_config` grip table (pinned by its
/// `gun_grips_aim_the_barrel_out_of_the_hand` test). So this one rotation aims
/// every VR weapon - ballistic, energy and psi alike - down its own rendered
/// barrel, with no per-weapon correction.
///
/// Caveat, pre-existing and unchanged by this: several *classic-install* world
/// models (`atek_w`, `ar15_w`, `sg_w`, `gren_w`, `viro_w`, `al_w`) are authored
/// barrel-along-Z instead, so VR mis-aims them by 90 degrees when the 25AE view
/// models are unavailable. Tracked in #1034.
fn barrel_axis_from_forward() -> Quaternion<f32> {
    Quaternion::from_angle_y(Deg(-90.0))
}

/// The weapon entity's current world position (from its live transform).
fn weapon_world_position(world: &World, entity_id: EntityId) -> Option<cgmath::Vector3<f32>> {
    let v_transform = world.borrow::<View<RuntimePropTransform>>().ok()?;
    let transform = v_transform.get(entity_id).ok()?;
    Some(transform.0.transform_point(point3(0.0, 0.0, 0.0)).to_vec())
}

use super::{
    Effect, Message, MessagePayload, Script,
    burst_fire::{BurstState, BurstStep},
    script_util::{
        active_gun_setting, get_all_links_with_template, ordered_projectile_links,
        play_environmental_sound,
    },
};

/// Get ammunition type from projectile template using pre-populated class tag map
fn get_ammotype_from_projectile_template(
    template_id: i32,
    class_tag_map: &std::collections::HashMap<i32, std::collections::HashMap<String, String>>,
) -> Option<String> {
    let template_tags = class_tag_map.get(&template_id)?;
    template_tags.get("ammotype").cloned()
}

/// Rounds one shot consumes in this fire setting: the laser pistol spends 3 per
/// normal shot and 20 per overcharge. A shot always costs at least one round,
/// so a setting a gun never authored cannot make it free to fire.
fn rounds_per_shot(setting: &GunSettingDesc) -> i32 {
    setting.ammo_usage.max(1)
}

/// Whether a magazine of `ammo` rounds can pay for a shot costing `rounds`.
/// `None` is a weapon that tracks no ammo at all (unlimited debug weapons).
fn can_pay_for_shot(ammo: Option<i32>, rounds: i32) -> bool {
    ammo.is_none_or(|ammo| ammo >= rounds)
}

/// The wait this fire setting imposes before the next pull may fire, in
/// seconds. Zero is a gun that fires as fast as the trigger is pulled.
fn shot_cooldown_seconds(setting: &GunSettingDesc) -> f32 {
    setting.shot_interval_ms as f32 / 1000.0
}

/// One per-shot multiplier of a fire setting, sanitized. The shipped data
/// leaves the modifiers of a setting the gun never authored at 0, which taken
/// literally would make its shots damageless and motionless - so only a
/// positive multiplier counts as one.
fn shot_multiplier(raw: f32) -> f32 {
    if raw.is_finite() && raw > 0.0 {
        raw
    } else {
        1.0
    }
}

/// The damage and speed multipliers this fire setting puts on the projectile it
/// launches (the EMP rifle's overcharge hits 3x; the fusion cannon's DEATH lob
/// travels at 0.4x).
fn shot_modifiers(setting: &GunSettingDesc) -> RuntimePropShotModifiers {
    RuntimePropShotModifiers {
        stim: shot_multiplier(setting.stim_modifier),
        speed: shot_multiplier(setting.speed_modifier),
    }
}

/// Whether `entity_id` is still inside the wait its last shot imposed.
fn is_cooling_down(world: &World, entity_id: EntityId) -> bool {
    world
        .borrow::<View<RuntimePropShotCooldown>>()
        .ok()
        .and_then(|v| v.get(entity_id).ok().map(|c| c.remaining > 0.0))
        .unwrap_or(false)
}

/// Whether a reload is running on this weapon - firing is blocked throughout.
fn is_reloading(world: &World, entity_id: EntityId) -> bool {
    world
        .borrow::<View<RuntimePropReloading>>()
        .ok()
        .and_then(|v| v.get(entity_id).ok().map(|r| !r.is_done()))
        == Some(true)
}

/// The rounds loaded in this weapon, or `None` for one that tracks no ammo at
/// all (the unlimited debug weapons).
fn loaded_ammo(world: &World, entity_id: EntityId) -> Option<i32> {
    world
        .borrow::<View<dark::properties::PropGunState>>()
        .ok()
        .and_then(|v| v.get(entity_id).ok().map(|g| g.ammo))
}

/// What one shot came to.
enum ShotOutcome {
    /// The gun fired: these effects are the shot.
    Fired(Effect),
    /// The magazine cannot pay for the shot.
    Empty,
    /// Not a gun at all - a flat-aimed melee weapon, whose trigger starts a
    /// swing instead of firing.
    FlatMeleeSwing,
}

pub struct WeaponScript {
    /// Shots the current trigger pull still owes, paid out one per frame by
    /// `update` (the pistol's BURST, the assault rifle's AUTO). Script-private
    /// and deliberately not saved: a save taken mid-burst loads with the
    /// trigger at rest, the same call `RuntimePropShotCooldown` makes.
    burst: Option<ActiveBurst>,
}

/// A burst in flight, with the setting the pull started in - so switching fire
/// mode part-way through cannot re-price the rounds it still owes.
struct ActiveBurst {
    setting: GunSettingDesc,
    state: BurstState,
}

impl WeaponScript {
    pub fn new() -> WeaponScript {
        WeaponScript { burst: None }
    }
}

impl Script for WeaponScript {
    /// Pay out the shots the current pull still owes. Nothing else about a
    /// weapon ticks per frame.
    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        time: &crate::time::Time,
    ) -> Effect {
        let Some(burst) = self.burst.as_mut() else {
            return Effect::NoEffect;
        };
        // A burst belongs to the pull that started it: a reload, or the gun
        // leaving the player's hands, ends it.
        if is_reloading(world, entity_id) || !crate::wielded_weapon::held_in_hand(world, entity_id)
        {
            self.burst = None;
            return Effect::NoEffect;
        }

        let setting = burst.setting.clone();
        let can_fire = can_pay_for_shot(loaded_ammo(world, entity_id), rounds_per_shot(&setting));
        match burst.state.advance(time.elapsed.as_secs_f32(), can_fire) {
            BurstStep::Wait => Effect::NoEffect,
            BurstStep::Done => {
                self.burst = None;
                Effect::NoEffect
            }
            BurstStep::Fire => match fire_one_shot(world, entity_id, &setting) {
                ShotOutcome::Fired(effect) => effect,
                // Unreachable in practice - `can_fire` covers the magazine and
                // a melee weapon authors no burst - but a burst that cannot
                // fire is over either way.
                ShotOutcome::Empty | ShotOutcome::FlatMeleeSwing => {
                    self.burst = None;
                    Effect::NoEffect
                }
            },
        }
    }

    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TriggerPull => {
                // Firing is blocked while a reload is in progress.
                if is_reloading(world, entity_id) {
                    return Effect::NoEffect;
                }

                // Everything below is governed by the gun's ACTIVE fire setting -
                // the mode the player has it switched to - so a laser pistol's
                // overcharge costs 20 rounds and waits 3 seconds where its
                // normal shot costs 3 and waits 350 ms. A weapon with no gun
                // description fires on the neutral default.
                let setting = active_gun_setting(world, entity_id).unwrap_or_default();

                // The wait the last shot imposed: a pull inside it does nothing
                // at all, not even the empty-clip click. Ahead of the melee
                // gate inside `fire_one_shot`, which is safe because only a
                // gunshot ever starts a cooldown.
                if is_cooling_down(world, entity_id) {
                    return Effect::NoEffect;
                }

                match fire_one_shot(world, entity_id, &setting) {
                    ShotOutcome::FlatMeleeSwing => Effect::FlatMeleeSwing { entity_id },
                    ShotOutcome::Empty => dry_fire(world, entity_id),
                    ShotOutcome::Fired(effect) => {
                        // A burst setting owes more rounds than the one just
                        // fired; `update` pays them out.
                        self.burst =
                            BurstState::begin(&setting).map(|state| ActiveBurst { setting, state });
                        effect
                    }
                }
            }
            MessagePayload::AnimationFlagTriggered { motion_flags }
                if motion_flags.contains(dark::motion::MotionFlags::TRIGGER1)
                    && ordered_projectile_links(world, entity_id).is_empty() =>
            {
                let Ok(aim) = world
                    .borrow::<View<RuntimePropFlatAim>>()
                    .unwrap()
                    .get(entity_id)
                    .copied()
                else {
                    return Effect::NoEffect;
                };
                flat_melee_hit(physics, aim, world)
            }
            MessagePayload::TriggerRelease => {
                // An unlimited burst ends with the trigger. A finite one plays
                // out regardless - letting go of the pistol's BURST one frame
                // in still sends all three rounds.
                if let Some(burst) = self.burst.as_mut() {
                    burst.state.release();
                }
                Effect::NoEffect
            }
            _ => Effect::NoEffect,
        }
    }
}

/// Fire one shot in `setting`: its sound, muzzle flash, projectile, ammo cost,
/// between-shots wait and the noise it makes. Shared by the trigger pull and by
/// the burst that pull leaves behind, so a burst round is in every way the shot
/// a pull would have fired. The caller has already cleared the reload and
/// cooldown gates, and decides what an `Empty` magazine means (a pull clicks; a
/// burst just stops).
fn fire_one_shot(world: &World, entity_id: EntityId, setting: &GunSettingDesc) -> ShotOutcome {
    //Create muzzle flash
    let muzzle_flashes = get_all_links_with_template(world, entity_id, |link| match link {
        Link::GunFlash(data) => Some(*data),
        _ => None,
    });

    // Pick the selected ammo type: guns carry several Projectile
    // links (standard / HE / AP, ...); RuntimePropSelectedAmmo indexes
    // into the ordered, setting-filtered list (absent = the first).
    let projectiles = ordered_projectile_links(world, entity_id);
    let selected_ammo = world
        .borrow::<View<RuntimePropSelectedAmmo>>()
        .ok()
        .and_then(|v| v.get(entity_id).ok().map(|s| s.0))
        .unwrap_or(0);
    let maybe_projectile = projectiles
        .get(selected_ammo % projectiles.len().max(1))
        .cloned();

    // A weapon with no Projectile link is melee. In flat mode the
    // trigger starts the authored player-arm swing; its MF_TRIGGER1
    // animation event resolves the short aimed raycast later, at
    // visible impact. (VR has no flat aim and damages through its
    // trigger-gated physical contact handler.)
    if maybe_projectile.is_none() {
        if world
            .borrow::<View<RuntimePropFlatAim>>()
            .unwrap()
            .get(entity_id)
            .is_ok()
        {
            return ShotOutcome::FlatMeleeSwing;
        }
    }

    // Ammo gating: weapons that carry a `PropGunState` are limited by
    // their clip. A magazine that cannot pay the setting's per-shot
    // cost dry-fires (no shot/flash). Weapons without a gun state
    // (e.g. unlimited debug weapons) are unaffected.
    let maybe_ammo = loaded_ammo(world, entity_id);
    let rounds = rounds_per_shot(setting);
    if !can_pay_for_shot(maybe_ammo, rounds) {
        return ShotOutcome::Empty;
    }

    // Include projectile class tags (ie, ammotype) and weaponmode for sound lookup
    let mut projectile_class_tags: Vec<(String, String)> =
        if let Some((projectile_template_id, _)) = &maybe_projectile {
            let class_tags = world
                .borrow::<UniqueView<GlobalTemplateClassTags>>()
                .unwrap();
            get_ammotype_from_projectile_template(*projectile_template_id, &class_tags.0)
                .map(|ammotype| vec![("ammotype".to_string(), ammotype)])
                .unwrap_or_default()
        } else {
            Vec::new()
        };

    // Add weaponmode=0 for shoot mode
    projectile_class_tags.push(("weaponmode".to_string(), "0".to_string()));

    let additional_sound_tags = projectile_class_tags
        .iter()
        .map(|(tag, value)| (tag.as_str(), value.as_str()))
        .collect::<Vec<_>>();

    let sound_effect = play_environmental_sound(
        world,
        entity_id,
        "shoot",
        additional_sound_tags,
        AudioHandle::new(),
    );

    // Only a real gunshot (a fired projectile) raises noise - a
    // projectile-less weapon that falls through the melee gate
    // must not emit a phantom gunshot.
    let is_gunshot = maybe_projectile.is_some();
    let projectile_effect = Effect::Multiple(
        maybe_projectile
            .into_iter()
            .map(|(template_id, options)| {
                create_projectile(
                    world,
                    entity_id,
                    template_id,
                    &options,
                    shot_modifiers(setting),
                )
            })
            .collect(),
    );

    let muzzle_flash_effect = Effect::Multiple(
        muzzle_flashes
            .into_iter()
            .map(|(template_id, options)| {
                create_muzzle_flash(world, entity_id, template_id, &options)
            })
            .collect(),
    );
    // let offset = obj_rotation * vec3(0.0128545, 0.5026805, -3.0933015) / SCALE_FACTOR;

    // let muzzle_flash_effect = Effect::CreateEntity {
    //     template_id: -2653,
    //     position: position + offset,
    //     orientation: *obj_rotation
    //         * Quaternion::from_axis_angle(vec3(0.0, 1.0, 0.0), Rad(PI / 2.0)),
    // };

    // Consume the setting's per-shot cost when the weapon tracks ammo.
    let mut effects = vec![sound_effect, muzzle_flash_effect, projectile_effect];
    if maybe_ammo.is_some() {
        effects.push(Effect::AdjustAmmo {
            entity_id,
            delta: -rounds,
        });
    }
    // Start the setting's between-shots wait. Only a real shot
    // starts one - a melee weapon that reached here has nothing to
    // pace.
    //
    // Every shot restarts it, burst rounds included: a burst pays
    // its rounds out on the shorter `burst_interval_ms` without
    // consulting the cooldown, so what is left when the burst ends
    // is the last round's full wait - the gap the setting asks for
    // between pulls.
    let cooldown = shot_cooldown_seconds(setting);
    if is_gunshot && cooldown > 0.0 {
        effects.push(Effect::BeginShotCooldown {
            entity_id,
            seconds: cooldown,
        });
    }
    // A gunshot is loud: nearby AIs hear it and investigate the
    // shooter, even without line of sight. The noise comes from
    // the weapon (in the player's hands), so its position stands
    // in for the shooter's.
    if is_gunshot {
        if let Some(origin) = weapon_world_position(world, entity_id) {
            effects.push(Effect::RaiseNoise {
                origin,
                radius: GUNSHOT_NOISE_RADIUS,
            });
        }
    }
    ShotOutcome::Fired(Effect::Multiple(effects))
}

/// Resolve the authored hit event of a flat melee swing: raycast a short
/// distance along the current crosshair ray and damage the hit entity (hitbox
/// proxies resolve to their parent).
fn flat_melee_hit(physics: &PhysicsWorld, aim: RuntimePropFlatAim, world: &World) -> Effect {
    // Flat melee is only ever the player's own swing, so Lethal Weapon
    // applies unconditionally here.
    let amount = crate::scripts::gui::lethal_weapon_damage(
        MELEE_DAMAGE,
        crate::scripts::gui::player_has_os_trait(world, crate::scripts::gui::TRAIT_LETHAL_WEAPON),
    );
    let hit = physics.ray_cast(
        aim.origin,
        aim.forward.normalize() * MELEE_RANGE,
        InternalCollisionGroups::ENTITIES
            | InternalCollisionGroups::HITBOX
            | InternalCollisionGroups::SELECTABLE,
    );
    if let Some(RayCastResult {
        maybe_entity_id: Some(target),
        hit_point,
        ..
    }) = hit
    {
        let target = resolve_proxy_entity(world, target);
        return Effect::Send {
            msg: Message {
                to: target,
                payload: MessagePayload::Damage {
                    // Adrenaline Overproduction scales the player's melee
                    // damage while it is active (1.0 otherwise).
                    amount: amount * crate::scripts::berserk::melee_damage_multiplier(world),
                    // Swing direction + contact point seed the victim's
                    // death-ragdoll reaction. No bone: melee resolves a hitbox
                    // proxy to its parent BEFORE sending (so HitBoxScript
                    // never stamps it) - the ragdoll's nearest-body-to-point
                    // fallback picks the struck limb from the contact point
                    // instead.
                    impact: Some(crate::scripts::DamageImpact {
                        direction: aim.forward.normalize(),
                        point: hit_point.to_vec(),
                        bone: None,
                    }),
                },
            },
        };
    }
    Effect::NoEffect
}

/// An empty-clip dry fire: no projectile or muzzle flash, just the weapon's
/// "dryfire" click (best-effort - resolves via the gun's sound schema, like the
/// "shoot" event). Reached when a `PropGunState` weapon has 0 rounds.
fn dry_fire(world: &World, entity_id: EntityId) -> Effect {
    play_environmental_sound(world, entity_id, "dryfire", vec![], AudioHandle::new())
}

pub(super) fn create_muzzle_flash(
    world: &World,
    entity_id: EntityId,
    muzzle_flash_template_id: i32,
    options: &GunFlashOptions,
) -> Effect {
    let v_transform = world.borrow::<View<RuntimePropTransform>>().unwrap();
    let v_vhots = world.borrow::<View<RuntimePropVhots>>().unwrap();

    let vhots = v_vhots
        .get(entity_id)
        .map(|vhots| vhots.0.clone())
        .unwrap_or_default();

    let vhot_offset = vhots
        .get(options.vhot as usize)
        .map(|v| v.point)
        .unwrap_or(point3(0.0, 0.0, 0.0));

    let transform = v_transform.get(entity_id).unwrap();

    let adjustments = vr_config::get_vr_hand_model_adjustments_from_entity(
        entity_id,
        world,
        vr_config::Handedness::Left,
    );
    let orientation = adjustments.rotation.invert() * Quaternion::from_angle_y(Deg(90.0));

    Effect::CreateEntity {
        template_id: muzzle_flash_template_id,
        position: vhot_offset,
        orientation,
        root_transform: transform.0,
        // Bolt the flash to the firing weapon so it tracks the (per-frame
        // re-placed) first-person viewmodel during its brief lifetime instead of
        // staying pinned at the fire-time pose.
        options: CreateEntityOptions {
            attach_to: Some(entity_id),
            ..CreateEntityOptions::default()
        },
    }
}

pub(super) fn create_projectile(
    world: &World,
    entity_id: EntityId,
    projectile_template_id: i32,
    _options: &ProjectileOptions,
    modifiers: RuntimePropShotModifiers,
) -> Effect {
    // Flatscreen camera-origin aim: if the weapon carries a flat fire ray, spawn
    // the projectile just ahead of the camera travelling straight along the
    // crosshair, ignoring the offset/rotated barrel transform. This makes both
    // hitscan bullets and slow physics projectiles (grenades) track the
    // crosshair. VR/AI weapons have no RuntimePropFlatAim and fall through.
    {
        let v_flat_aim = world.borrow::<View<RuntimePropFlatAim>>().unwrap();
        if let Ok(aim) = v_flat_aim.get(entity_id) {
            let origin = aim.origin + aim.forward.normalize() * FLAT_MUZZLE_CLEARANCE;
            // get_rotation_from_forward_vector puts `forward` in the +Z column,
            // and projectile velocity is root_transform * (0,0,mag) - so the
            // shot travels along the crosshair ray.
            let rot: Matrix4<f32> =
                get_rotation_from_forward_vector(aim.forward.normalize()).into();
            return Effect::CreateEntity {
                template_id: projectile_template_id,
                position: point3(0.0, 0.0, 0.0),
                orientation: Quaternion::from_angle_y(Deg(90.0)),
                root_transform: Matrix4::from_translation(origin.to_vec()) * rot,
                options: CreateEntityOptions {
                    force_visible: true,
                    projectile_raycast_origin: Some(aim.origin),
                    shot_modifiers: Some(modifiers),
                    ..CreateEntityOptions::default()
                },
            };
        }
    }

    // VR: the weapon is a physical object in the hand, so the shot is defined
    // entirely by the *rendered* weapon - it leaves the model's muzzle vhot
    // travelling down the model's barrel. Both are read from the weapon's own
    // live transform, so no per-weapon aim correction is involved.
    let v_transform = world.borrow::<View<RuntimePropTransform>>().unwrap();
    let v_vhots = world.borrow::<View<RuntimePropVhots>>().unwrap();

    let vhots = v_vhots
        .get(entity_id)
        .map(|vhots| vhots.0.clone())
        .unwrap_or_default();
    // The fire point is the model's first vhot, which the 25AE view models that
    // carry one author at the -X tip of the barrel (atek_h, ar15_h, sg_h,
    // lasehand, sfg_h, viro_h, amp_h).
    //
    // Documented fallback for a model with no vhot at all: the model origin,
    // i.e. the grip. The shot still leaves along the barrel, just from the
    // hand. This is not rare - `empgun_h`, `gren_h`, `fsn_h` and `al_h` ship
    // with zero vhots, as do most classic-install world models - and a shot
    // starting at the grip can strike the player when the weapon is held in
    // close to the body (see #1034). Adding clearance is deliberately left to
    // that issue: it changes where four more weapons fire from, which is a
    // separate change from making the vhot-carrying weapons faithful.
    let muzzle = vhots
        .first()
        .map(|v| v.point)
        .unwrap_or(point3(0.0, 0.0, 0.0));

    let transform = v_transform.get(entity_id).unwrap();

    Effect::CreateEntity {
        template_id: projectile_template_id,
        position: point3(0.0, 0.0, 0.0),
        // HACK: Not sure why we need to do this, but seems projectile
        // models are rotated 90 degrees
        orientation: Quaternion::from_angle_y(Deg(90.0)),
        // Projectile velocity is `root_transform * (0, 0, magnitude)`, so put
        // the muzzle at the origin and aim +Z down the barrel.
        root_transform: transform.0
            * Matrix4::from_translation(muzzle.to_vec())
            * Matrix4::from(barrel_axis_from_forward()),
        options: CreateEntityOptions {
            force_visible: true,
            shot_modifiers: Some(modifiers),
            ..CreateEntityOptions::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use cgmath::{Quaternion, point3, vec3};
    use dark::{motion::MotionFlags, properties::Links};

    use crate::physics::{CollisionGroup, PhysicsWorld};

    use super::*;

    fn flat_melee_fixture() -> (World, PhysicsWorld, EntityId, EntityId) {
        let mut world = World::new();
        let weapon = world.add_entity((
            Links::empty(),
            RuntimePropFlatAim {
                origin: point3(0.0, 0.0, 0.0),
                forward: vec3(0.0, 0.0, 1.0),
            },
        ));
        let target = world.add_entity(());

        let mut physics = PhysicsWorld::new();
        physics.add_kinematic(
            target,
            vec3(0.0, 0.0, 0.6),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(0.2, 0.2, 0.2),
            CollisionGroup::selectable(),
            false,
        );
        // Populate Rapier's broad phase before the first query, as the real
        // mission loop does each frame.
        let player = world.add_entity(());
        let mut player_handle = physics.create_player(vec3(100.0, 100.0, 100.0), player);
        physics.update(vec3(0.0, 0.0, 0.0), &mut player_handle);

        assert!(
            includes_damage_to(
                flat_melee_hit(
                    &physics,
                    RuntimePropFlatAim {
                        origin: point3(0.0, 0.0, 0.0),
                        forward: vec3(0.0, 0.0, 1.0),
                    },
                    &world,
                ),
                target,
            ),
            "fixture must put a damageable target inside melee reach"
        );

        (world, physics, weapon, target)
    }

    fn includes_damage_to(effect: Effect, target: EntityId) -> bool {
        Effect::flatten(vec![effect]).into_iter().any(|effect| {
            matches!(
                effect,
                Effect::Send { msg }
                    if msg.to == target
                        && matches!(msg.payload, MessagePayload::Damage { .. })
            )
        })
    }

    /// Where a VR shot starts and which way it flies, in world space, from the
    /// `CreateEntity` the script returns.
    fn vr_fire_geometry(
        transform: Matrix4<f32>,
        vhots: Vec<dark::ss2_bin_obj_loader::Vhot>,
    ) -> (cgmath::Vector3<f32>, cgmath::Vector3<f32>) {
        let mut world = World::new();
        let weapon = world.add_entity((
            Links::empty(),
            RuntimePropTransform(transform),
            RuntimePropVhots(vhots),
        ));

        let effect = create_projectile(
            &world,
            weapon,
            -1,
            &dark::properties::ProjectileOptions {
                order: 0,
                setting: 0,
            },
            RuntimePropShotModifiers::default(),
        );
        let Effect::CreateEntity {
            position,
            root_transform,
            ..
        } = effect
        else {
            panic!("firing must create the projectile entity");
        };
        // Projectile velocity is root_transform * (0, 0, magnitude).
        let origin = root_transform.transform_point(position).to_vec();
        let forward = root_transform.transform_vector(vec3(0.0, 0.0, 1.0));
        (origin, forward.normalize())
    }

    fn muzzle_vhot(point: cgmath::Point3<f32>) -> dark::ss2_bin_obj_loader::Vhot {
        dark::ss2_bin_obj_loader::Vhot {
            vhot_type: dark::ss2_bin_obj_loader::VhotType::Unknown,
            point,
        }
    }

    /// A VR shot leaves the model's muzzle vhot travelling down the barrel
    /// (the model's -X), for any pose the hand happens to hold the weapon in.
    #[test]
    fn a_vr_shot_leaves_the_muzzle_vhot_along_the_barrel() {
        // A deliberately awkward pose: yawed, pitched and translated, so a
        // wrong fire axis cannot coincide with the right one.
        let rotation = Quaternion::from_angle_y(Deg(35.0)) * Quaternion::from_angle_x(Deg(-20.0));
        let translation = vec3(3.0, 1.5, -4.0);
        let transform = Matrix4::from_translation(translation) * Matrix4::from(rotation);
        let vhot = point3(-0.77, -0.03, 0.06);

        let (origin, forward) = vr_fire_geometry(transform, vec![muzzle_vhot(vhot)]);

        let expected_origin = translation + rotation * vhot.to_vec();
        let expected_forward = rotation * vec3(-1.0, 0.0, 0.0);
        assert!(
            (origin - expected_origin).magnitude() < 1e-5,
            "shot started at {origin:?}, not at the muzzle {expected_origin:?}"
        );
        assert!(
            (forward - expected_forward).magnitude() < 1e-5,
            "shot flew {forward:?}, not down the barrel {expected_forward:?}"
        );
    }

    /// A model with no vhot at all (the classic `laser` / `lasehand` meshes)
    /// falls back to the model origin, still firing down the barrel.
    #[test]
    fn a_vr_shot_from_a_vhotless_model_falls_back_to_the_model_origin() {
        let rotation = Quaternion::from_angle_y(Deg(-90.0));
        let transform = Matrix4::from_translation(vec3(1.0, 2.0, 3.0)) * Matrix4::from(rotation);

        let (origin, forward) = vr_fire_geometry(transform, vec![]);

        assert!((origin - vec3(1.0, 2.0, 3.0)).magnitude() < 1e-5);
        assert!((forward - rotation * vec3(-1.0, 0.0, 0.0)).magnitude() < 1e-5);
    }

    /// The laser pistol: 3 rounds a normal shot, 20 an overcharge.
    fn laser_setting(ammo_usage: i32, shot_interval_ms: u32) -> GunSettingDesc {
        GunSettingDesc {
            ammo_usage,
            shot_interval_ms,
            ..GunSettingDesc::default()
        }
    }

    #[test]
    fn a_shot_costs_the_settings_ammo_usage() {
        assert_eq!(rounds_per_shot(&laser_setting(3, 350)), 3, "laser NORM");
        assert_eq!(rounds_per_shot(&laser_setting(20, 3000)), 20, "laser OVER");
        // A setting the gun never authored must not make it free to fire.
        assert_eq!(rounds_per_shot(&laser_setting(0, 0)), 1);
    }

    #[test]
    fn a_magazine_must_cover_the_whole_shot() {
        assert!(can_pay_for_shot(Some(3), 3), "exactly enough still fires");
        assert!(!can_pay_for_shot(Some(2), 3), "a partial charge dry-fires");
        assert!(!can_pay_for_shot(Some(0), 1));
        assert!(
            can_pay_for_shot(None, 20),
            "a weapon with no clip is unlimited"
        );
    }

    #[test]
    fn the_cooldown_is_the_settings_shot_interval() {
        assert_eq!(shot_cooldown_seconds(&laser_setting(3, 350)), 0.35);
        assert_eq!(shot_cooldown_seconds(&laser_setting(20, 3000)), 3.0);
        assert_eq!(shot_cooldown_seconds(&laser_setting(1, 0)), 0.0);
    }

    #[test]
    fn only_a_positive_multiplier_modifies_a_shot() {
        let emp_over = GunSettingDesc {
            stim_modifier: 3.0,
            speed_modifier: 0.8,
            ..GunSettingDesc::default()
        };
        let modifiers = shot_modifiers(&emp_over);
        assert_eq!(modifiers.stim, 3.0);
        assert_eq!(modifiers.speed, 0.8);

        // The uninitialized record every gun carries for a mode it does not
        // have: zeroes must not silently disarm the shot.
        let unauthored = GunSettingDesc {
            stim_modifier: 0.0,
            speed_modifier: 0.0,
            ..GunSettingDesc::default()
        };
        let modifiers = shot_modifiers(&unauthored);
        assert_eq!(modifiers.stim, 1.0);
        assert_eq!(modifiers.speed, 1.0);
    }

    #[test]
    fn flat_melee_trigger_starts_the_animation_without_hitting_early() {
        let (world, physics, weapon, target) = flat_melee_fixture();

        let effect = WeaponScript::new().handle_message(
            weapon,
            &world,
            &physics,
            &MessagePayload::TriggerPull,
        );

        assert!(
            !includes_damage_to(effect, target),
            "the trigger edge is only the start of the visible swing"
        );
    }

    #[test]
    fn flat_melee_hits_on_the_authored_player_swing_trigger() {
        let (world, physics, weapon, target) = flat_melee_fixture();

        let effect = WeaponScript::new().handle_message(
            weapon,
            &world,
            &physics,
            &MessagePayload::AnimationFlagTriggered {
                motion_flags: MotionFlags::TRIGGER1,
            },
        );

        assert!(
            includes_damage_to(effect, target),
            "leftswing's MF_TRIGGER1 frame must resolve the aimed melee hit"
        );
    }
}
