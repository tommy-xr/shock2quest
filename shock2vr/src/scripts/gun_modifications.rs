//! What a gun's modification level does to the way it fires.
//!
//! Every modifiable gun authors its own modify script - `PistolModify`,
//! `ShotgunModify`, and eight more - and each of those scripts is a fixed set
//! of changes to the gun's firing description, announced to the player by the
//! gun's `P$Modify1`/`P$Modify2` text ("Increase clip size from 12 to 24.").
//! There is no per-gun *behaviour* to run, only per-gun numbers, so the ten
//! scripts are one table here rather than ten script objects; the names stay
//! registered in [`crate::scripts`] so the authored script still resolves.
//!
//! The numbers are applied where the firing description is *read*
//! ([`crate::scripts::script_util::active_gun_setting`]), never written into
//! the gun's `PropBaseGunDesc`. That keeps the authored archetype the one
//! source of truth: the modification is a function of `PropGunState.modification`
//! - which already saves and loads - so a modified gun comes back modified with
//! nothing to migrate, and re-deriving it any number of times cannot compound.

use dark::properties::{GunSettingDesc, PropGunState, PropScripts};
use shipyard::{EntityId, Get, View, World};

/// The highest modification a gun can reach. Retail refuses a third attempt
/// outright rather than wasting the nanites.
pub const MAX_MODIFICATION: i32 = 2;

/// A gun's firing description at one modification level, as multipliers on the
/// values the gun authors. Each level is expressed against the *unmodified*
/// gun, not against the level below it, because that is how the shipped
/// numbers read ("damage up to 25% over the standard") and because deriving
/// from the base is what makes re-deriving harmless.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Modification {
    /// Magazine size.
    clip: f32,
    /// Time to reload. Below one is faster.
    reload: f32,
    /// Ammo units per shot. Below one is thriftier.
    ammo_usage: f32,
    /// Projectile speed.
    speed: f32,
    /// Projectile stimulus - the gun's damage.
    damage: f32,
}

impl Modification {
    /// The unmodified gun: every value as authored.
    const NONE: Modification = Modification {
        clip: 1.0,
        reload: 1.0,
        ammo_usage: 1.0,
        speed: 1.0,
        damage: 1.0,
    };
}

/// Reducing a reload or an ammo cost "by 2/3rds" leaves a third of it.
const TWO_THIRDS_OFF: f32 = 1.0 / 3.0;
/// Taking a third off leaves two thirds of it. Only the grenade launcher's
/// second modification reduces by a third; every other reduction here is a
/// half or two thirds.
const ONE_THIRD_OFF: f32 = 2.0 / 3.0;
/// Every gun's first modification adds a tenth to its damage, and its second
/// takes that to a quarter over the unmodified gun.
const DAMAGE_1: f32 = 1.1;
const DAMAGE_2: f32 = 1.25;

/// One gun's two modification levels, in order.
struct WeaponModifications {
    /// The gun's authored modify script, which is what identifies it.
    script: &'static str,
    levels: [Modification; MAX_MODIFICATION as usize],
}

/// Build a level from the unmodified gun, reading as the shipped text does.
const fn level(clip: f32, reload: f32, ammo_usage: f32, speed: f32, damage: f32) -> Modification {
    Modification {
        clip,
        reload,
        ammo_usage,
        speed,
        damage,
    }
}

/// The ten modifiable guns and what each modification does to them.
///
/// The effects are the ones the guns' own `MODIFY1.STR`/`MODIFY2.STR` text
/// announces; where that text gives no magnitude the documented community
/// figures fill it in (cited in this change's pull request). Two entries the
/// shipped data settles outright: the pistol's clip goes 12 to 24 and the
/// assault rifle's 36 to 72, both exactly a doubling. Clip increases the text
/// does quantify are taken at their word instead - the grenade launcher's 6 to
/// 9 and the laser's and EMP rifle's charge are half again, not double - and an
/// unquantified "increase clip size" is read as a doubling.
///
/// Damage is not per-gun: every modifiable gun that does damage at all gets the
/// same [`DAMAGE_1`]/[`DAMAGE_2`] buff at each level, whether or not its own
/// text mentions it. The stasis field generator is the one exception, because
/// it does no damage to scale.
const MODIFICATIONS: [WeaponModifications; 10] = [
    // Pistol: clip 12 -> 24, then a reload cut to a third.
    WeaponModifications {
        script: "PistolModify",
        levels: [
            level(2.0, 1.0, 1.0, 1.0, DAMAGE_1),
            level(2.0, TWO_THIRDS_OFF, 1.0, 1.0, DAMAGE_2),
        ],
    },
    // Assault rifle: the same two effects in the other order, clip 36 -> 72.
    WeaponModifications {
        script: "RifleModify",
        levels: [
            level(1.0, TWO_THIRDS_OFF, 1.0, 1.0, DAMAGE_1),
            level(2.0, TWO_THIRDS_OFF, 1.0, 1.0, DAMAGE_2),
        ],
    },
    // Shotgun: a reload cut, then less kick. The kick lives in the unparsed
    // `P$GunKick`, so the second modification is damage only for now.
    WeaponModifications {
        script: "ShotgunModify",
        levels: [
            level(1.0, TWO_THIRDS_OFF, 1.0, 1.0, DAMAGE_1),
            level(1.0, TWO_THIRDS_OFF, 1.0, 1.0, DAMAGE_2),
        ],
    },
    // Laser pistol: charge to 150%, then half of what a shot draws, as its
    // own text promises. Three units a shot rounds to two.
    WeaponModifications {
        script: "LaserModify",
        levels: [
            level(1.5, 1.0, 1.0, 1.0, DAMAGE_1),
            level(1.5, 1.0, 0.5, 1.0, DAMAGE_2),
        ],
    },
    // EMP rifle: charge to 150% and a faster shot, then half the draw.
    WeaponModifications {
        script: "EMPModify",
        levels: [
            level(1.5, 1.0, 1.0, 1.5, DAMAGE_1),
            level(1.5, 1.0, 0.5, 1.5, DAMAGE_2),
        ],
    },
    // Grenade launcher: clip 6 -> 9, then faster grenades and a shorter reload.
    WeaponModifications {
        script: "GrenadeModify",
        levels: [
            level(1.5, 1.0, 1.0, 1.0, DAMAGE_1),
            level(1.5, ONE_THIRD_OFF, 1.0, 1.5, DAMAGE_2),
        ],
    },
    // Stasis field generator: half again the shot speed, as its own text
    // promises, then half the prisms. It does no damage, so the blanket damage
    // buff has nothing to scale and neither level carries one.
    WeaponModifications {
        script: "StasisModify",
        levels: [
            level(1.0, 1.0, 1.0, 1.5, 1.0),
            level(1.0, 1.0, 0.5, 1.5, 1.0),
        ],
    },
    // Fusion cannon: clip 40 -> 80, then one prism a shot instead of two.
    WeaponModifications {
        script: "FusionModify",
        levels: [
            level(2.0, 1.0, 1.0, 1.0, DAMAGE_1),
            level(2.0, 1.0, 0.5, 1.0, DAMAGE_2),
        ],
    },
    // Annelid (worm) launcher: a bigger clip, then twice the projectile speed.
    WeaponModifications {
        script: "AnnelidModify",
        levels: [
            level(2.0, 1.0, 1.0, 1.0, DAMAGE_1),
            level(2.0, 1.0, 1.0, 2.0, DAMAGE_2),
        ],
    },
    // Viral proliferator: a bigger clip, then half the worms a shot.
    WeaponModifications {
        script: "ViralModify",
        levels: [
            level(2.0, 1.0, 1.0, 1.0, DAMAGE_1),
            level(2.0, 1.0, 0.5, 1.0, DAMAGE_2),
        ],
    },
];

/// The modification table `weapon` is modified by, or `None` for a gun that
/// authors no modify script and therefore cannot be modified at all.
///
/// The gun's scripts are borrowed once and matched against all ten entries,
/// rather than borrowing per entry - this is called for every read of a gun's
/// firing description.
fn table_for(world: &World, weapon: EntityId) -> Option<&'static WeaponModifications> {
    let scripts = world.borrow::<View<PropScripts>>().ok()?;
    let scripts = scripts.get(weapon).ok()?;
    MODIFICATIONS.iter().find(|entry| {
        scripts
            .scripts
            .iter()
            .any(|script| script.eq_ignore_ascii_case(entry.script))
    })
}

/// Whether `weapon` authors a modify script - the data's own answer to "is
/// this a gun modification does anything to".
pub fn has_modify_script(world: &World, weapon: EntityId) -> bool {
    table_for(world, weapon).is_some()
}

/// `weapon`'s current modification level, clamped to what a gun can reach.
pub fn modification_level(world: &World, weapon: EntityId) -> i32 {
    world
        .borrow::<View<PropGunState>>()
        .ok()
        .and_then(|states| states.get(weapon).ok().map(|state| state.modification))
        .unwrap_or(0)
        .clamp(0, MAX_MODIFICATION)
}

/// The modification `weapon` is currently carrying, as multipliers on what it
/// authors. The unmodified gun - and any gun with no modify script - gets
/// [`Modification::NONE`], which leaves every value alone.
fn current(world: &World, weapon: EntityId) -> Modification {
    let level = modification_level(world, weapon);
    if level <= 0 {
        return Modification::NONE;
    }
    table_for(world, weapon)
        .and_then(|entry| entry.levels.get(level as usize - 1).copied())
        .unwrap_or(Modification::NONE)
}

/// Scale a count that has to stay whole and at least one - a magazine, or the
/// rounds a shot costs. A value that is not positive to begin with is a
/// setting the gun does not author (the third, unused fire setting every gun
/// ships carries zeroes) and is left exactly as it is: rounding it up to one
/// would invent a magazine where the data has none.
fn scale_count(value: i32, by: f32) -> i32 {
    if value <= 0 {
        return value;
    }
    ((value as f32) * by).round().max(1.0) as i32
}

/// `setting` as a gun modified to `level` fires it.
fn apply(setting: &GunSettingDesc, modification: Modification) -> GunSettingDesc {
    GunSettingDesc {
        clip: scale_count(setting.clip, modification.clip),
        ammo_usage: scale_count(setting.ammo_usage, modification.ammo_usage),
        reload_time_ms: ((setting.reload_time_ms as f32) * modification.reload).round() as u32,
        speed_modifier: setting.speed_modifier * modification.speed,
        stim_modifier: setting.stim_modifier * modification.damage,
        ..setting.clone()
    }
}

/// `setting`, as `weapon`'s modification level has it fire. This is the one
/// place a modification takes effect; every reader of a gun's firing
/// description goes through
/// [`active_gun_setting`](crate::scripts::script_util::active_gun_setting),
/// which calls it.
pub fn modified_setting(
    world: &World,
    weapon: EntityId,
    setting: &GunSettingDesc,
) -> GunSettingDesc {
    apply(setting, current(world, weapon))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped pistol's first fire setting, and the shipped rifle's.
    fn pistol_setting() -> GunSettingDesc {
        GunSettingDesc {
            burst: 1,
            clip: 12,
            spray: 1,
            stim_modifier: 1.0,
            burst_interval_ms: 0,
            shot_interval_ms: 500,
            ammo_usage: 1,
            speed_modifier: 1.0,
            reload_time_ms: 500,
        }
    }

    fn gun(scripts: &[&str], modification: i32) -> (World, EntityId) {
        let mut world = World::new();
        let gun = world.add_entity((
            PropScripts {
                scripts: scripts.iter().map(|s| (*s).to_owned()).collect(),
                inherits: true,
            },
            PropGunState {
                ammo: 0,
                condition: 100.0,
                setting: 0,
                modification,
                silence_value: 0.0,
            },
        ));
        (world, gun)
    }

    /// The shipped text names the pistol's numbers outright: clip 12 to 24 at
    /// the first modification, a reload cut to a third at the second, and the
    /// clip stays doubled - each level describes the whole gun, not a step.
    #[test]
    fn the_pistol_gains_a_clip_then_a_faster_reload() {
        for (level, clip, reload) in [(0, 12, 500), (1, 24, 500), (2, 24, 167)] {
            let (world, weapon) = gun(&["PistolModify", "WeaponScript"], level);
            let modified = modified_setting(&world, weapon, &pistol_setting());
            assert_eq!((modified.clip, modified.reload_time_ms), (clip, reload));
        }
    }

    /// Damage is a tenth over the unmodified gun at the first modification and
    /// a quarter over it at the second - not a tenth compounded onto a quarter.
    #[test]
    fn damage_is_measured_against_the_unmodified_gun() {
        let base = pistol_setting();
        for (level, expected) in [(0, 1.0), (1, 1.1), (2, 1.25)] {
            let (world, weapon) = gun(&["PistolModify"], level);
            let modified = modified_setting(&world, weapon, &base);
            assert!((modified.stim_modifier - expected).abs() < 1e-5);
        }
    }

    /// The rifle takes the pistol's two effects in the other order, and its
    /// shipped text names the same doubling: 36 to 72.
    #[test]
    fn the_rifle_gains_a_faster_reload_then_a_clip() {
        let base = GunSettingDesc {
            clip: 36,
            reload_time_ms: 1000,
            ..pistol_setting()
        };
        for (level, clip, reload) in [(0, 36, 1000), (1, 36, 333), (2, 72, 333)] {
            let (world, weapon) = gun(&["RifleModify"], level);
            let modified = modified_setting(&world, weapon, &base);
            assert_eq!((modified.clip, modified.reload_time_ms), (clip, reload));
        }
    }

    /// The shotgun's kick lives in a property nothing parses yet, so its
    /// second modification currently only adds damage - but it must not
    /// silently undo the first one's reload cut.
    #[test]
    fn the_shotgun_keeps_its_faster_reload_at_the_second_modification() {
        let base = GunSettingDesc {
            reload_time_ms: 1000,
            ..pistol_setting()
        };
        let (world, weapon) = gun(&["ShotgunModify"], 2);
        let modified = modified_setting(&world, weapon, &base);
        assert_eq!(modified.reload_time_ms, 333);
        assert!((modified.stim_modifier - DAMAGE_2).abs() < 1e-5);
    }

    /// The laser stores half again as much charge, then draws half as much per
    /// shot - three units a shot rounding to two, the nearest whole draw.
    #[test]
    fn the_laser_gains_charge_then_efficiency() {
        let base = GunSettingDesc {
            clip: 100,
            ammo_usage: 3,
            reload_time_ms: 0,
            ..pistol_setting()
        };
        for (level, clip, usage) in [(0, 100, 3), (1, 150, 3), (2, 150, 2)] {
            let (world, weapon) = gun(&["LaserModify"], level);
            let modified = modified_setting(&world, weapon, &base);
            assert_eq!((modified.clip, modified.ammo_usage), (clip, usage));
        }
    }

    /// The stasis generator's own text promises half again the shot speed, and
    /// no damage change at all - it does none.
    #[test]
    fn the_stasis_generator_gains_shot_speed_then_efficiency() {
        let base = GunSettingDesc {
            clip: 12,
            ammo_usage: 4,
            speed_modifier: 0.6,
            reload_time_ms: 0,
            ..pistol_setting()
        };
        for (level, speed, usage) in [(0, 0.6, 4), (1, 0.9, 4), (2, 0.9, 2)] {
            let (world, weapon) = gun(&["StasisModify"], level);
            let modified = modified_setting(&world, weapon, &base);
            assert!((modified.speed_modifier - speed).abs() < 1e-5);
            assert_eq!(modified.ammo_usage, usage);
            assert!((modified.stim_modifier - 1.0).abs() < 1e-5);
        }
    }

    /// The unused third fire setting every gun ships authors zeroes, and the
    /// psi amp authors a clipless, ammo-free setting outright. Scaling must
    /// leave those alone rather than inventing a one-round magazine or making
    /// a free shot cost a point of ammo.
    #[test]
    fn a_setting_the_gun_does_not_author_is_left_alone() {
        let unauthored = GunSettingDesc {
            burst: 0,
            clip: 0,
            ammo_usage: 0,
            reload_time_ms: 0,
            ..pistol_setting()
        };
        for level in 0..=2 {
            // A gun whose table would otherwise double the clip and, at the
            // second level, shorten the reload.
            let (world, weapon) = gun(&["PistolModify"], level);
            let modified = modified_setting(&world, weapon, &unauthored);
            assert_eq!((modified.clip, modified.ammo_usage), (0, 0));
            assert_eq!(modified.reload_time_ms, 0);
            // And the psi amp, which authors no modify script at all.
            let (world, amp) = gun(&["PsiAmpScript"], level);
            assert_eq!(modified_setting(&world, amp, &unauthored), unauthored);
        }
    }

    /// A gun that authors no modify script is left exactly as authored, even
    /// if something has set a modification level on it.
    #[test]
    fn a_gun_with_no_modify_script_is_never_altered() {
        let (world, weapon) = gun(&["WeaponScript"], 2);
        assert!(!has_modify_script(&world, weapon));
        assert_eq!(
            modified_setting(&world, weapon, &pistol_setting()),
            pistol_setting()
        );
    }

    /// Deriving from the level rather than writing the gun's archetype is what
    /// makes a modification survive a save and never compound: the same
    /// authored setting read again gives the same answer.
    #[test]
    fn re_deriving_a_modification_gives_the_same_gun() {
        let (world, weapon) = gun(&["PistolModify"], 2);
        let once = modified_setting(&world, weapon, &pistol_setting());
        let twice = modified_setting(&world, weapon, &pistol_setting());
        assert_eq!(once, twice);
        assert_eq!(once.clip, 24);
    }

    /// A level past the second - which nothing should ever write - reads as
    /// the second rather than falling off the table into an unmodified gun.
    #[test]
    fn a_level_past_the_last_reads_as_the_last() {
        let (world, weapon) = gun(&["PistolModify"], 7);
        assert_eq!(modification_level(&world, weapon), MAX_MODIFICATION);
        assert_eq!(modified_setting(&world, weapon, &pistol_setting()).clip, 24);
    }
}
