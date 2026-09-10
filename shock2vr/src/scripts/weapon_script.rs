use cgmath::{Deg, EuclideanSpace, InnerSpace, Matrix4, Quaternion, Rotation3, Transform, point3};
use dark::{
    SCALE_FACTOR,
    properties::{
        GunFlashOptions, GunSettingDesc, Link, ObjectState, ProjectileOptions, PropGunReliability,
        PropWeaponType,
    },
};
use engine::audio::AudioHandle;
use rand::Rng;
use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{
    mission::{
        entity_creator::CreateEntityOptions,
        mission_core::{GlobalSkillParams, GlobalTemplateClassTags},
    },
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
        active_gun_setting, get_all_links_with_template, gun_condition, ordered_projectile_links,
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

/// The condition points this weapon loses per shot, or `None` for one that
/// authors no reliability (melee weapons, the debug weapons) and so never
/// wears.
fn degrade_per_shot(world: &World, entity_id: EntityId) -> Option<f32> {
    let rate = world
        .borrow::<View<dark::properties::PropGunReliability>>()
        .ok()
        .and_then(|v| v.get(entity_id).ok().map(|r| r.degrade_rate))?;
    (rate > 0.0).then_some(rate)
}

/// Whether the Anti-entropic Field is running: while it is, a gun neither
/// breaks nor wears. The power's own casting behaviour is still being sorted
/// out (#1304) - this only asks whether it is active.
fn is_weapon_stability_active(world: &World) -> bool {
    world
        .borrow::<UniqueView<crate::psi::ActivePsiPowers>>()
        .is_ok_and(|active| active.is_active(crate::psi::STABILITY_TEMPLATE_ID))
}

/// The player's skill level for the class of weapon `entity_id` belongs to.
/// The class is authored on the weapon archetypes and inherited by every gun
/// under them (0 conventional, 1 energy, 2 heavy, 3 annelid); a gun that
/// authors none counts as conventional, as the retail engine does.
fn weapon_skill_level(world: &World, entity_id: EntityId) -> i32 {
    use crate::player_stats::Skill;

    let skill = match world
        .borrow::<View<PropWeaponType>>()
        .ok()
        .and_then(|v| v.get(entity_id).ok().map(|t| t.0))
        .unwrap_or(0)
    {
        1 => Skill::EnergyWeapons,
        2 => Skill::HeavyWeapons,
        3 => Skill::ExoticWeapons,
        // 4 is the psi amp, which has no gun skill and never wears.
        _ => Skill::StandardWeapons,
    };

    crate::scripts::script_util::player_skill_level(world, skill)
}

/// Dark CalcRandAngle: authored error per missing weapon-skill level. Stock
/// SKILLPARAM sets this to zero. Strength and Sharpshooter do not enter this
/// calculation (Sharpshooter is a stimulus multiplier in the original).
fn weapon_inaccuracy(world: &World, entity_id: EntityId) -> u16 {
    let per_level = world
        .borrow::<UniqueView<GlobalSkillParams>>()
        .ok()
        .and_then(|params| params.0.as_ref().map(|p| p.inaccuracy_degrees))
        .unwrap_or(0.0);
    let degrees = crate::player_stats::PlayerStats::shot_deviation_degrees(
        per_level,
        weapon_skill_level(world, entity_id),
    );
    (degrees * (65536.0 / 360.0)) as u16
}

/// The chance, 0..1, that the shot about to be fired breaks the gun. Zero at
/// or above the reliability's break threshold: a gun in good condition cannot
/// break at all. Below the threshold the authored min/max percentages are
/// interpolated over the remaining condition, and the player's skill with that
/// class of weapon scales the result down by the gamesys break factor.
///
/// The interpolation is the engine's own, and it is blunter than the two
/// authored numbers suggest: the span is a *hundred* condition points while
/// the threshold sits at ten, so a gun that has just crossed the threshold is
/// already at its authored maximum (the pistol: 5% a shot) and only creeps
/// past it as the last points go. `min_break` is effectively the value the
/// curve would reach a hundred points below the threshold - i.e. never.
fn break_chance(
    reliability: &PropGunReliability,
    condition: f32,
    break_factor: f32,
    weapon_skill: i32,
) -> f32 {
    if condition >= reliability.thresh_break {
        return 0.0;
    }
    let min = reliability.min_break / 100.0;
    let max = reliability.max_break / 100.0;
    let wear = 1.0 - (condition - reliability.thresh_break) / 100.0;
    let skill_relief = 1.0 - break_factor * weapon_skill as f32;
    ((min + wear * (max - min)) * skill_relief).clamp(0.0, 1.0)
}

/// Roll this shot against the gun's break chance. `Some` = the gun just broke:
/// it goes to `Broken` (which stops it firing until it is repaired) and plays
/// its authored break event.
fn roll_for_breakage(world: &World, entity_id: EntityId) -> Option<Effect> {
    if is_weapon_stability_active(world) {
        return None;
    }
    let reliability = world
        .borrow::<View<PropGunReliability>>()
        .ok()
        .and_then(|v| v.get(entity_id).ok().copied())?;
    let condition = gun_condition(world, entity_id)?;
    let break_factor = world
        .borrow::<UniqueView<GlobalSkillParams>>()
        .ok()
        .and_then(|params| params.0.as_ref().map(|params| params.weapon_break_factor))
        .unwrap_or(0.0);
    let chance = break_chance(
        &reliability,
        condition,
        break_factor,
        weapon_skill_level(world, entity_id),
    );

    (rand::thread_rng().r#gen::<f32>() < chance).then(|| {
        Effect::Multiple(vec![
            Effect::SetObjectState {
                entity_id,
                state: ObjectState::Broken,
            },
            play_environmental_sound(world, entity_id, "break", vec![], AudioHandle::new()),
            Effect::ShowMessage {
                text: weapon_breaks_message(world, entity_id),
            },
        ])
    })
}

/// The status line a gun posts as it gives out: MISC.STR's `WeaponBreaks`
/// ("%s has broken!") with the gun's short name in it.
fn weapon_breaks_message(world: &World, entity_id: EntityId) -> String {
    let name = crate::scripts::script_util::object_short_name(world, entity_id).unwrap_or_default();
    crate::hud::hud_strings(world)
        .weapon_breaks_message
        .replace("%s", &name)
}

/// What one shot came to.
enum ShotOutcome {
    /// The gun fired: these effects are the shot.
    Fired(Effect),
    /// The magazine cannot pay for the shot.
    Empty,
    /// The gun is not in working order (broken by wear, or by a botched
    /// repair): it cannot fire at all until it is repaired.
    NotWorking,
    /// This shot broke the gun instead of firing it.
    Broke(Effect),
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
        if is_reloading(world, entity_id)
            || !crate::wielded_weapon::held_in_hand(world, entity_id)
            || crate::weapon_requirements::unmet_weapon_skill(world, entity_id).is_some()
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
                ShotOutcome::Empty | ShotOutcome::FlatMeleeSwing | ShotOutcome::NotWorking => {
                    self.burst = None;
                    Effect::NoEffect
                }
                // The burst broke the gun: it owes no more rounds.
                ShotOutcome::Broke(effect) => {
                    self.burst = None;
                    effect
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
                if let Some(requirement) =
                    crate::weapon_requirements::unmet_weapon_skill(world, entity_id)
                {
                    self.burst = None;
                    return Effect::ShowWeaponSkillRequirement {
                        entity_id,
                        requirement,
                    };
                }
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
                    ShotOutcome::NotWorking => play_environmental_sound(
                        world,
                        entity_id,
                        "broken",
                        vec![],
                        AudioHandle::new(),
                    ),
                    ShotOutcome::Broke(effect) => effect,
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
            // The maintenance tool, offered to this weapon on the port's tool
            // channel: a VR hand releasing it onto the gun, or the flat
            // use-mode click on a carried one. The decision lives here, on the
            // receiving weapon, because everything it turns on - the gun's
            // condition, its broken state, the Maintain level it authors - is
            // the weapon's own, and because a gun runs no GuiScript that could
            // claim the offer the way the security crate claims an ICE Pick.
            MessagePayload::ProvideForConsumption { entity }
                if crate::scripts::maintenance::is_maintenance_tool(world, *entity) =>
            {
                crate::scripts::maintenance::apply(world, *entity, Some(entity_id))
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

    // Only a real gunshot (a fired projectile) wears the gun, raises
    // noise or can break it - a projectile-less weapon that falls
    // through the melee gate must not emit a phantom gunshot.
    let is_gunshot = maybe_projectile.is_some();

    // A gun that is not in working order refuses to fire, ahead of the
    // magazine: an empty broken gun is broken, not empty. The engine refuses
    // on any state but Normal; the gate is narrower here because the annelid
    // weapons ship Unresearched and the port cannot research them yet, so the
    // wider gate would leave them permanently unusable.
    if is_gunshot
        && matches!(
            crate::scripts::gui::object_state(world, entity_id),
            ObjectState::Broken | ObjectState::Destroyed
        )
    {
        return ShotOutcome::NotWorking;
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

    // Firing wears the gun: each gunshot costs the authored per-shot
    // condition points. Guns with no reliability authored (and debug
    // weapons) never wear, and neither does a projectile-less pull.
    let wear = (is_gunshot && !is_weapon_stability_active(world))
        .then(|| degrade_per_shot(world, entity_id))
        .flatten()
        .map(|amount| Effect::AdjustWeaponCondition {
            entity_id,
            delta: -amount,
        });

    // A worn gun can break as the trigger comes back, spending the shot
    // without firing it. The shot still wears the gun down.
    if is_gunshot {
        if let Some(broke) = roll_for_breakage(world, entity_id) {
            return ShotOutcome::Broke(Effect::Multiple(
                std::iter::once(broke).chain(wear).collect(),
            ));
        }
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
    effects.extend(wear);
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
                    amount: MELEE_DAMAGE * crate::scripts::berserk::melee_damage_multiplier(world),
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
/// authored "OutofAmmo" event (Dark cPlayerGun::PullTrigger). This also covers
/// a mode whose per-shot cost exceeds the remaining ammo.
fn dry_fire(world: &World, entity_id: EntityId) -> Effect {
    play_environmental_sound(world, entity_id, "outofammo", vec![], AudioHandle::new())
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
        .iter()
        .find(|vhot| vhot.id == options.vhot)
        .map(|v| v.point)
        .unwrap_or_else(|| crate::weapon_muzzle::resolve(world, entity_id).point);

    let transform = v_transform.get(entity_id).unwrap();

    // GunFlash flag 1 means launch a physical object (e.g. an ejected casing),
    // not an effect that remains attached to the gun. Dark CreateGunFlashes.
    if options.flags & 1 != 0 {
        let mut attachment = crate::weapon_muzzle::resolve(world, entity_id);
        attachment.point = vhot_offset;
        let frame = attachment.shot_frame(transform.0);
        // Flat stores its right-handed viewmodel in the logical left slot.
        let left = !world
            .borrow::<View<RuntimePropFlatAim>>()
            .unwrap()
            .contains(entity_id)
            && crate::wielded_weapon::weapon_in_hand(world, vr_config::Handedness::Left)
                == Some(entity_id);
        let velocity_frame =
            frame * Matrix4::from_nonuniform_scale(if left { -1.0 } else { 1.0 }, 1.0, 1.0);
        return Effect::CreateEntity {
            template_id: muzzle_flash_template_id,
            position: point3(0.0, 0.0, 0.0),
            // Casing meshes are long along +Y. Lay that axis along the
            // barrel (+Z in this launch frame), not upright above the gun.
            orientation: Quaternion::from_angle_x(Deg(90.0)),
            root_transform: frame,
            options: CreateEntityOptions {
                launch_projectile: true,
                authored_velocity_frame: Some(velocity_frame),
                transient_fx: true,
                player_fired_projectile: true,
                projectile_weapon: Some(entity_id),
                projectile_launch_origin: Some(transform.0.transform_point(point3(0.0, 0.0, 0.0))),
                ..CreateEntityOptions::default()
            },
        };
    }

    // Flash meshes are authored along -X, just like held gun barrels. The old
    // inverse grip adjustment turned them 180 degrees, putting their one-sided
    // faces away from the shooter and the cone back inside the gun.
    let axis = crate::weapon_muzzle::resolve(world, entity_id).axis;
    let orientation = Quaternion::from_arc(cgmath::vec3(-1.0, 0.0, 0.0), axis, None);

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

/// Spawn a shot from a weapon **the player is firing**.
///
/// This is the player's fire path only: the sole production senders of
/// `TriggerPull` are `virtual_hand` (the VR hand) and `flat_player_controller`,
/// so every projectile created here belongs to the player. AI and turret shots
/// never reach this function - `ai_util` emits its own `Effect::CreateEntity`.
/// That is why the shot is unconditionally marked `player_fired_projectile`
/// below, which makes it transparent to the player's own capsule; an AI shot,
/// being unmarked, still hits the player normally.
pub(super) fn create_projectile(
    world: &World,
    entity_id: EntityId,
    projectile_template_id: i32,
    _options: &ProjectileOptions,
    modifiers: RuntimePropShotModifiers,
) -> Effect {
    let spray = world
        .borrow::<UniqueView<crate::mission::projectile_spray::GlobalProjectileSprays>>()
        .ok()
        .and_then(|sprays| sprays.0.get(&projectile_template_id).copied())
        .unwrap_or_default();
    let mut rng = rand::thread_rng();
    let launch = crate::mission::projectile_spray::deviate(
        create_projectile_launch(world, entity_id, projectile_template_id, modifiers),
        weapon_inaccuracy(world, entity_id),
        &mut rng,
    );
    crate::mission::projectile_spray::expand(spray, launch, &mut rng)
}

fn create_projectile_launch(
    world: &World,
    entity_id: EntityId,
    projectile_template_id: i32,
    modifiers: RuntimePropShotModifiers,
) -> Effect {
    // Flatscreen camera-origin aim: if the weapon carries a flat fire ray, spawn
    // the projectile just ahead of the camera travelling straight along the
    // crosshair, ignoring the offset/rotated barrel transform. This makes both
    // hitscan bullets and slow physics projectiles (grenades) track the
    // crosshair. A VR weapon has no RuntimePropFlatAim and falls through.
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
                    projectile_weapon: Some(entity_id),
                    projectile_raycast_origin: Some(aim.origin),
                    projectile_launch_origin: Some(aim.origin),
                    shot_modifiers: Some(modifiers),
                    player_fired_projectile: true,
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
    let muzzle = crate::weapon_muzzle::resolve(world, entity_id);

    let transform = v_transform.get(entity_id).unwrap();

    let shot_frame = muzzle.shot_frame(transform.0);

    Effect::CreateEntity {
        template_id: projectile_template_id,
        position: point3(0.0, 0.0, 0.0),
        // HACK: Not sure why we need to do this, but seems projectile
        // models are rotated 90 degrees
        orientation: Quaternion::from_angle_y(Deg(90.0)),
        // Projectile velocity is `root_transform * (0, 0, magnitude)`, so put
        // the muzzle at the origin and aim +Z down the barrel.
        root_transform: shot_frame,
        options: CreateEntityOptions {
            shot_modifiers: Some(modifiers),
            player_fired_projectile: true,
            projectile_launch_origin: Some(transform.0.transform_point(point3(0.0, 0.0, 0.0))),
            projectile_weapon: Some(entity_id),
            ..CreateEntityOptions::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use cgmath::Rotation;

    #[test]
    fn authored_accuracy_uses_weapon_skill_not_strength_or_sharpshooter() {
        use crate::{mission::mission_core::GlobalSkillParams, quest_info::QuestInfo};
        use shipyard::UniqueViewMut;
        let mut world = World::new();
        let gun = world.add_entity(PropWeaponType(0));
        world.add_unique(QuestInfo::new());
        world.add_unique(GlobalSkillParams(Some(dark::gamesys::SkillParams {
            inaccuracy_degrees: 0.703125, // 128 Dark angle units; synthetic fixture
            weapon_break_factor: 0.0,
            research_factor: 0.0,
            damage_modifier: 0.0,
            organ_damage: 0.0,
        })));
        for strength in [1, 6] {
            for (skill, expected) in [(1, 640), (3, 384), (6, 0)] {
                {
                    let mut quests = world.borrow::<UniqueViewMut<QuestInfo>>().unwrap();
                    let stats = quests.player_stats_mut();
                    stats.strength = strength;
                    stats.skills.standard_weapons = skill;
                }
                assert_eq!(weapon_inaccuracy(&world, gun), expected);
                world
                    .borrow::<UniqueViewMut<QuestInfo>>()
                    .unwrap()
                    .player_stats_mut()
                    .add_os_trait(5);
                assert_eq!(weapon_inaccuracy(&world, gun), expected);
            }
        }
        // The stock record explicitly disables random weapon aim error, even
        // at minimum skill. This must not invent a cone for the shipping game.
        world
            .borrow::<UniqueViewMut<GlobalSkillParams>>()
            .unwrap()
            .0
            .as_mut()
            .unwrap()
            .inaccuracy_degrees = 0.0;
        world
            .borrow::<UniqueViewMut<QuestInfo>>()
            .unwrap()
            .player_stats_mut()
            .skills
            .standard_weapons = 1;
        assert_eq!(weapon_inaccuracy(&world, gun), 0);
    }

    #[test]
    fn flash_cone_points_out_of_held_and_world_barrels() {
        for (name, axis) in [
            ("ar15_h", vec3(-1.0, 0.0, 0.0)),
            ("ar15_w", vec3(0.0, 0.0, -1.0)),
        ] {
            let mut world = World::new();
            let gun = world.add_entity((
                dark::properties::PropModelName(name.to_owned()),
                RuntimePropTransform(Matrix4::from_scale(0.55)),
                RuntimePropVhots(vec![muzzle_vhot(point3(-1.0, 0.0, 0.0))]),
            ));
            let Effect::CreateEntity { orientation, .. } =
                create_muzzle_flash(&world, gun, -2653, &GunFlashOptions { vhot: 0, flags: 0 })
            else {
                panic!("flash must spawn");
            };
            // The assflash cone extends down local -X. Flipping it points the
            // visible CW front away from the shooter and buries it in the gun.
            let forward = orientation.rotate_vector(vec3(-1.0, 0.0, 0.0));
            assert!(
                (forward - axis).magnitude() < 0.001,
                "{name}: cone faces {forward:?}"
            );
        }
    }

    #[test]
    fn gun_flash_resolves_a_sparse_authored_vhot_id() {
        let mut world = World::new();
        let point = point3(-1.5, 0.2, 0.3);
        let weapon = world.add_entity((
            dark::properties::PropModelName("test_weapon".to_owned()),
            RuntimePropTransform(Matrix4::from_scale(1.0)),
            RuntimePropVhots(vec![
                dark::ss2_bin_obj_loader::Vhot { id: 8, point },
                muzzle_vhot(point3(-0.1, 0.0, 0.0)),
            ]),
        ));
        for (id, expected) in [
            (8, point),
            (0, point3(-0.1, 0.0, 0.0)),
            (1, point3(-0.1, 0.0, 0.0)),
        ] {
            let Effect::CreateEntity { position, .. } =
                create_muzzle_flash(&world, weapon, -1, &GunFlashOptions { vhot: id, flags: 0 })
            else {
                panic!("a GunFlash link must create an effect");
            };
            assert_eq!(
                position, expected,
                "GunFlash names an ID, not a vector index"
            );
        }
    }

    #[test]
    fn an_untrained_trigger_returns_only_the_authored_requirement() {
        let (mut world, physics, weapon, _) = flat_melee_fixture();
        world.add_component(weapon, dark::properties::PropBaseWeaponDesc([6, 0, 0, 0]));
        let effect = WeaponScript::new().handle_message(
            weapon,
            &world,
            &physics,
            &MessagePayload::TriggerPull,
        );
        assert!(matches!(effect, Effect::ShowWeaponSkillRequirement {
            entity_id,
            requirement: crate::weapon_requirements::WeaponSkillRequirement {
                required: 6, current: 0, ..
            },
        } if entity_id == weapon));
    }

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
        (origin, forward)
    }

    #[test]
    fn supported_weapon_shots_follow_the_solved_barrel_in_both_hands() {
        use crate::vr_support::solve_two_hand_pose;
        for hand in [crate::Handedness::Left, crate::Handedness::Right] {
            let base = Quaternion::from_angle_y(Deg(70.0));
            let primary = vec3(1.0, 2.0, 3.0);
            let anchor = hand
                .gun_mirror()
                .transform_point(point3(-0.6, -0.1, 0.04))
                .to_vec();
            let primary_anchor = vec3(0.02, 0.0, 0.0);
            let support =
                primary + base.rotate_vector(anchor - primary_anchor) + vec3(0.0, 0.15, 0.08);
            let solved = solve_two_hand_pose(primary, support, base, primary_anchor, anchor, base);
            let transform = Matrix4::from_translation(solved.position)
                * Matrix4::from(solved.rotation)
                * Matrix4::from_scale(0.7);
            let muzzle = hand.gun_mirror().transform_point(point3(-0.9, 0.1, 0.03));
            let (origin, velocity) = vr_fire_geometry(transform, vec![muzzle_vhot(muzzle)]);
            assert!((origin - transform.transform_point(muzzle).to_vec()).magnitude() < 1e-5);
            let forward = solved.rotation.rotate_vector(vec3(-1.0, 0.0, 0.0));
            assert!(velocity.normalize().dot(forward) > 0.99999);
            assert!(
                (forward - base.rotate_vector(vec3(-1.0, 0.0, 0.0))).magnitude() > 0.05,
                "support must actually steer the shot away from the single-hand direction"
            );
        }
    }

    #[test]
    fn scaled_weapons_move_the_muzzle_without_scaling_projectile_speed() {
        let rotation = Matrix4::from_angle_y(Deg(31.0)) * Matrix4::from_angle_x(Deg(-12.0));
        for scale in [0.4, 0.7, 1.3] {
            let transform = Matrix4::from_translation(vec3(3.0, 4.0, 5.0))
                * rotation
                * Matrix4::from_scale(scale);
            let muzzle = point3(-0.8, 0.1, 0.03);
            let (origin, velocity) = vr_fire_geometry(transform, vec![muzzle_vhot(muzzle)]);
            assert!((origin - transform.transform_point(muzzle).to_vec()).magnitude() < 1e-5);
            assert!((velocity.magnitude() - 1.0).abs() < 1e-5);
            assert!(
                (velocity - rotation.transform_vector(vec3(-1.0, 0.0, 0.0))).magnitude() < 1e-5
            );
        }
    }

    fn muzzle_vhot(point: cgmath::Point3<f32>) -> dark::ss2_bin_obj_loader::Vhot {
        dark::ss2_bin_obj_loader::Vhot { id: 0, point }
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

        let (origin, forward) = vr_fire_geometry(
            transform,
            vec![
                dark::ss2_bin_obj_loader::Vhot {
                    id: 8,
                    point: point3(0.0, 0.0, 0.0),
                },
                muzzle_vhot(vhot),
            ],
        );

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

    /// The pistol's shipped reliability: it cannot break above 10 condition,
    /// and below that it breaks on roughly 5% of shots.
    fn pistol_reliability() -> PropGunReliability {
        PropGunReliability {
            min_break: 0.5,
            max_break: 5.0,
            degrade_rate: 1.0,
            thresh_break: 10.0,
        }
    }

    /// The break chance across the condition range, for a player with no skill
    /// in the weapon: nothing at all while the gun is in good condition, and
    /// the authored maximum once it wears past the threshold.
    #[test]
    fn break_chance_over_the_condition_range() {
        let r = pistol_reliability();
        for (condition, expected) in [
            // Well above the threshold a gun cannot break.
            (100.0, 0.0),
            (10.5, 0.0),
            // The threshold itself is still safe: the roll wants condition
            // strictly below it.
            (10.0, 0.0),
            // Just past it the chance is already the authored maximum...
            (9.9, 0.050045),
            // ...and creeps a little higher as the last condition goes.
            (0.0, 0.0545),
        ] {
            let chance = break_chance(&r, condition, 0.0, 0);
            assert!(
                (chance - expected).abs() < 1.0e-5,
                "condition {condition}: expected {expected}, got {chance}",
            );
        }
    }

    /// Skill with the weapon holds it together: the gamesys break factor scales
    /// the chance down per level of skill, and can only ever remove it.
    #[test]
    fn weapon_skill_reduces_the_break_chance() {
        let r = pistol_reliability();
        let unskilled = break_chance(&r, 0.0, 0.1, 0);
        let skilled = break_chance(&r, 0.0, 0.1, 3);

        assert!(skilled < unskilled, "skill must make breakage less likely");
        assert!((skilled - unskilled * 0.7).abs() < 1.0e-6);
        assert_eq!(break_chance(&r, 0.0, 0.1, 20), 0.0);
    }

    /// A loaded gun with one standard ammo link, in the object state given.
    fn gun_world(state: Option<ObjectState>) -> (World, EntityId) {
        use dark::properties::{Links, PropGunState, PropObjState, ToLink};

        let mut world = World::new();
        let gun = world.add_entity((
            PropGunState {
                ammo: 12,
                condition: 100.0,
                setting: 0,
                modification: 0,
                silence_value: 0.0,
            },
            Links {
                to_links: vec![ToLink {
                    link: Link::Projectile(ProjectileOptions {
                        order: 0,
                        setting: -1,
                    }),
                    to_entity_id: None,
                    to_template_id: -1,
                }],
            },
            RuntimePropTransform(Matrix4::from_scale(1.0)),
        ));
        if let Some(state) = state {
            world.add_component(gun, PropObjState(state));
        }
        world.add_unique(GlobalTemplateClassTags(Default::default()));
        (world, gun)
    }

    /// A gun that gives out says which gun it was, in the shipped wording.
    #[test]
    fn the_break_message_names_the_gun() {
        let (mut world, gun) = gun_world(None);
        world.add_component(gun, dark::properties::PropSymName("Pistol".to_owned()));
        // No strings table loaded, so this exercises the English fallback the
        // format itself carries - the substitution is what is under test.
        assert_eq!(weapon_breaks_message(&world, gun), "Pistol has broken!");
    }

    /// A broken gun is out of the fight until it is repaired: the trigger
    /// clicks and nothing leaves the barrel.
    #[test]
    fn a_broken_gun_does_not_fire() {
        let (world, gun) = gun_world(Some(ObjectState::Broken));

        assert!(
            matches!(
                fire_one_shot(&world, gun, &GunSettingDesc::default()),
                ShotOutcome::NotWorking
            ),
            "a broken gun must refuse the shot"
        );

        // The same gun in working order fires, so the refusal is the object
        // state and not the fixture.
        let (world, gun) = gun_world(Some(ObjectState::Normal));
        assert!(matches!(
            fire_one_shot(&world, gun, &GunSettingDesc::default()),
            ShotOutcome::Fired(_)
        ));
    }

    /// A gun that authors no object state at all is in working order.
    #[test]
    fn a_gun_without_an_object_state_fires() {
        let (world, gun) = gun_world(None);

        assert!(matches!(
            fire_one_shot(&world, gun, &GunSettingDesc::default()),
            ShotOutcome::Fired(_)
        ));
    }

    /// A shot that breaks the gun: it does not fire - no projectile, no ammo
    /// spent - but it still wears the gun down, as the engine does.
    #[test]
    fn a_breaking_shot_marks_the_gun_broken_and_does_not_fire() {
        let (mut world, gun) = gun_world(Some(ObjectState::Normal));
        // A worn gun whose reliability always breaks it: it is below the
        // threshold and the chance saturates at 1.
        world.add_component(
            gun,
            dark::properties::PropGunState {
                ammo: 12,
                condition: 50.0,
                setting: 0,
                modification: 0,
                silence_value: 0.0,
            },
        );
        world.add_component(
            gun,
            PropGunReliability {
                min_break: 100.0,
                max_break: 100.0,
                degrade_rate: 1.0,
                thresh_break: 100.0,
            },
        );

        let ShotOutcome::Broke(effect) = fire_one_shot(&world, gun, &GunSettingDesc::default())
        else {
            panic!("a gun that always breaks must break on the shot");
        };

        let effects = Effect::flatten(vec![effect]);
        assert!(
            effects.iter().any(|effect| matches!(
                effect,
                Effect::SetObjectState {
                    state: ObjectState::Broken,
                    ..
                }
            )),
            "the gun must go to Broken"
        );
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::AdjustWeaponCondition { .. })),
            "the breaking shot still wears the gun"
        );
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::CreateEntity { .. })),
            "nothing leaves the barrel"
        );
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::AdjustAmmo { .. })),
            "and the round is not spent"
        );
    }
}
