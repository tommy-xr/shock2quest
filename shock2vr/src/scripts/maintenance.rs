//! The maintenance tool (gamesys template -2949, `P$Scripts ["Wrench"]`, class
//! tag `device2type mainttool`).
//!
//! Applying one to a ranged weapon restores condition points and uses the tool
//! up. How many points is the player's Maintain skill, ten points a level, and
//! a weapon never reads better than new. Each weapon also authors the minimum
//! Maintain level it can be worked on at, in `P$RequiredTechDesc`; the tool
//! refuses - and survives - below it, as it does on something that is not a
//! ranged weapon at all, on a weapon that has already broken (that wants the
//! repair skill), and on one already in good condition.
//!
//! The offer arrives on the port's tool channel,
//! [`MessagePayload::ProvideForConsumption`](crate::scripts::MessagePayload),
//! which is sent *to the target*: a VR hand releasing the tool onto a gun, or
//! the flat use-mode click on a carried tool. The decision therefore lives on
//! the receiving weapon (`WeaponScript`), keyed off the tool's authored script
//! the way the security crate recognizes an ICE Pick.

use dark::properties::{ObjectState, PropGunState, PropStackCount};
use engine::audio::AudioHandle;
use shipyard::{EntityId, Get, View, World};

use crate::player_stats::Skill;
use crate::scripts::Effect;

/// The maintenance tool's authored script name (`P$Scripts` on gamesys
/// template -2949). Identifying the tool by its authored script - not by a
/// hardcoded template id - keeps any object the data marks as one working.
const MAINTENANCE_TOOL_SCRIPT: &str = "Wrench";

/// Condition points one level of Maintain restores.
const POINTS_PER_MAINTAIN_LEVEL: f32 = 10.0;

/// Full condition: the ceiling a restore clamps to, and the level at which the
/// tool refuses as unnecessary.
const FULL_CONDITION: f32 = 100.0;

/// The sound the tool makes when it is used, from the `mainttool` class tag's
/// activation schema.
const MAINTENANCE_TOOL_SOUND: &str = "act_mainttool";

/// Whether `entity_id` is a maintenance tool.
pub fn is_maintenance_tool(world: &World, entity_id: EntityId) -> bool {
    crate::scripts::script_util::entity_has_script(world, entity_id, MAINTENANCE_TOOL_SCRIPT)
}

/// What using a maintenance tool on a target comes to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MaintenanceOutcome {
    /// `weapon` is worked on, gaining `points` condition (already capped at
    /// what full condition leaves room for). Naming the weapon here is what
    /// makes a success the only outcome that can be acted on.
    Restored { weapon: EntityId, points: f32 },
    /// The target is not a ranged weapon.
    NotAGun,
    /// The weapon has broken; maintenance cannot bring it back.
    Broken,
    /// The player's Maintain skill is below the level the weapon authors.
    SkillRequired { level: i32 },
    /// The weapon is already at full condition.
    AlreadyGood,
}

/// The minimum Maintain level `weapon` can be worked on at, or 0 for one that
/// authors no requirement.
/// The minimum Maintain level `weapon` can be worked on at. At least one: the
/// tool restores ten points a level, so a level-zero player working a weapon
/// that authors no requirement would spend it on nothing at all.
fn required_maintain_level(world: &World, weapon: EntityId) -> i32 {
    crate::scripts::script_util::required_tech_level(world, weapon, |tech| tech.maintenance())
        .max(1)
}

/// Decide what the tool does to `target`. `None` is "the gesture named no
/// target at all", which reads the same as pointing it at something that is
/// not a weapon.
pub fn maintenance_outcome(world: &World, target: Option<EntityId>) -> MaintenanceOutcome {
    let Some(target) = target else {
        return MaintenanceOutcome::NotAGun;
    };
    // The psi amp inherits a gun state whose condition means nothing, and
    // authors its own script in place of the weapon one that would take this
    // offer - so it is not a ranged weapon for this purpose.
    let Some(condition) = world
        .borrow::<View<PropGunState>>()
        .ok()
        .and_then(|v| v.get(target).ok().map(|gun| gun.condition))
        .filter(|_| !crate::wielded_weapon::is_psi_amp(world, target))
    else {
        return MaintenanceOutcome::NotAGun;
    };
    // Checked before the skill and the condition: a broken weapon is a repair
    // job whatever the player's Maintain level, and its condition may well
    // still be above zero.
    if matches!(
        crate::scripts::gui::object_state(world, target),
        ObjectState::Broken | ObjectState::Destroyed
    ) {
        return MaintenanceOutcome::Broken;
    }
    let required = required_maintain_level(world, target);
    let skill = crate::scripts::script_util::player_skill_level(world, Skill::Maintenance);
    if skill < required {
        return MaintenanceOutcome::SkillRequired { level: required };
    }
    if condition >= FULL_CONDITION {
        return MaintenanceOutcome::AlreadyGood;
    }
    let points = (skill as f32 * POINTS_PER_MAINTAIN_LEVEL).min(FULL_CONDITION - condition);
    MaintenanceOutcome::Restored {
        weapon: target,
        points,
    }
}

/// The line a refusal puts on the status channel.
fn refusal_message(world: &World, outcome: MaintenanceOutcome) -> String {
    let strings = crate::hud::hud_strings(world);
    match outcome {
        MaintenanceOutcome::NotAGun => strings.wrench_on_non_gun,
        MaintenanceOutcome::Broken => strings.wrench_on_broken,
        MaintenanceOutcome::SkillRequired { level } => {
            strings.wrench_skill_req.replace("%d", &level.to_string())
        }
        MaintenanceOutcome::AlreadyGood => strings.wrench_unused,
        // A restore is not a refusal; `apply` matches it first.
        MaintenanceOutcome::Restored { .. } => strings.wrench_on_non_gun,
    }
}

/// Use one of `tool` up. The tool stacks (`P$StackCoun`), so a stack of more
/// than one loses a unit and only the last one destroys the object.
fn consume_tool(world: &World, tool: EntityId) -> Effect {
    let stack = world
        .borrow::<View<PropStackCount>>()
        .ok()
        .and_then(|v| v.get(tool).ok().map(|s| s.0))
        .unwrap_or(1);
    if stack > 1 {
        Effect::AdjustStackCount {
            entity_id: tool,
            delta: -1,
        }
    } else {
        Effect::DestroyEntity { entity_id: tool }
    }
}

/// Apply `tool` to `target` - the whole gesture, from either presentation:
/// the condition it restores and the tool it uses up, or the line explaining
/// why it did neither.
pub fn apply(world: &World, tool: EntityId, target: Option<EntityId>) -> Effect {
    match maintenance_outcome(world, target) {
        MaintenanceOutcome::Restored { weapon, points } => Effect::combine(vec![
            Effect::AdjustWeaponCondition {
                entity_id: weapon,
                delta: points,
            },
            consume_tool(world, tool),
            Effect::PlaySound {
                handle: AudioHandle::new(),
                source: Some(weapon),
                name: MAINTENANCE_TOOL_SOUND.to_owned(),
                spatial: false,
            },
        ]),
        outcome => Effect::ShowMessage {
            text: refusal_message(world, outcome),
        },
    }
}

/// Whether releasing `tool` against `target` is a maintenance gesture at all -
/// the guard the VR two-hand release uses before offering the tool to what the
/// other hand holds. A refusable weapon still counts: the player gets told
/// why, which is the feedback the gesture owes them.
pub fn offers_to(world: &World, tool: EntityId, target: EntityId) -> bool {
    is_maintenance_tool(world, tool)
        && !matches!(
            maintenance_outcome(world, Some(target)),
            MaintenanceOutcome::NotAGun
        )
}

/// What using one carried item does: open the repair board on a broken gun,
/// wield a working weapon, apply a maintenance tool to the weapon already
/// wielded, or Frob anything else where it stands.
///
/// This is the whole of the flat inventory's use gesture - the strip's
/// double-click and the backpack panel's click, which are the same action and
/// share it. Flat has no way to drag one carried item onto another (#817), so
/// the wielded weapon is the one target a tool can name; the tool is applied
/// here rather than offered over the message channel, so a target that runs no
/// weapon script still gets its refusal instead of silence.
pub fn use_carried_item(world: &World, item: EntityId) -> Effect {
    // A broken gun is not wielded: it opens the repair board instead, which is
    // the only thing that can put it back into working order.
    if let Some(effect) = crate::scripts::gui::use_broken_weapon(world, item) {
        return effect;
    }
    if crate::virtual_hand::is_wieldable_weapon(world, item) {
        return Effect::GrabEntity {
            entity_id: item,
            hand: crate::vr_config::Handedness::Right,
            current_parent_id: None,
        };
    }
    if is_maintenance_tool(world, item) {
        return apply(world, item, crate::wielded_weapon::wielded_weapon(world));
    }
    Effect::Send {
        msg: crate::scripts::Message {
            payload: crate::scripts::MessagePayload::Frob,
            to: item,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dark::properties::{PropObjState, PropScripts};

    /// A pistol at `condition`, the tool, and a player with `maintain` levels
    /// of Maintain. The pistol authors the shipped requirement (Maintain 1).
    fn fixture(condition: f32, maintain: i32) -> (World, EntityId, EntityId) {
        let mut world = World::new();
        let gun = world.add_entity((
            PropGunState {
                ammo: 12,
                condition,
                setting: 0,
                modification: 0,
                silence_value: 0.0,
            },
            dark::properties::PropRequiredTechDesc(dark::properties::TechSkillValues([
                0, 1, 1, 1, 0,
            ])),
        ));
        let tool = world.add_entity((PropScripts {
            scripts: vec!["Wrench".to_owned()],
            inherits: true,
        },));
        let mut quests = crate::quest_info::QuestInfo::new();
        for _ in 0..maintain {
            quests.player_stats_mut().raise_skill(Skill::Maintenance);
        }
        world.add_unique(quests);
        (world, gun, tool)
    }

    fn effects(effect: Effect) -> Vec<Effect> {
        Effect::flatten(vec![effect])
    }

    #[test]
    fn the_tool_is_recognized_by_its_authored_script() {
        let (world, gun, tool) = fixture(50.0, 1);
        assert!(is_maintenance_tool(&world, tool));
        assert!(!is_maintenance_tool(&world, gun));
    }

    #[test]
    fn maintaining_restores_ten_points_per_maintain_level() {
        let (world, gun, _) = fixture(40.0, 1);
        assert_eq!(
            maintenance_outcome(&world, Some(gun)),
            MaintenanceOutcome::Restored {
                weapon: gun,
                points: 10.0
            }
        );

        let (world, gun, _) = fixture(40.0, 3);
        assert_eq!(
            maintenance_outcome(&world, Some(gun)),
            MaintenanceOutcome::Restored {
                weapon: gun,
                points: 30.0
            }
        );
    }

    #[test]
    fn a_restore_never_takes_a_weapon_past_full_condition() {
        let (world, gun, _) = fixture(95.0, 4);
        assert_eq!(
            maintenance_outcome(&world, Some(gun)),
            MaintenanceOutcome::Restored {
                weapon: gun,
                points: 5.0
            }
        );
    }

    #[test]
    fn using_the_tool_on_a_non_weapon_refuses() {
        let (mut world, _, _) = fixture(40.0, 1);
        let rock = world.add_entity((PropObjState(ObjectState::Normal),));
        assert_eq!(
            maintenance_outcome(&world, Some(rock)),
            MaintenanceOutcome::NotAGun
        );
        assert_eq!(
            maintenance_outcome(&world, None),
            MaintenanceOutcome::NotAGun
        );
    }

    /// The two-hand release guard: only a maintenance tool, and only against
    /// something that can be maintained. A weapon that will refuse still
    /// counts - the player is owed the reason.
    #[test]
    fn the_two_hand_guard_admits_only_a_tool_and_a_weapon() {
        let (mut world, gun, tool) = fixture(40.0, 1);
        let rock = world.add_entity((PropObjState(ObjectState::Normal),));

        assert!(offers_to(&world, tool, gun));
        assert!(!offers_to(&world, tool, rock), "a tool needs a weapon");
        assert!(!offers_to(&world, rock, gun), "only the tool applies");

        world.add_component(gun, (PropObjState(ObjectState::Broken),));
        assert!(
            offers_to(&world, tool, gun),
            "a broken weapon still earns the refusal line"
        );
    }

    #[test]
    fn a_broken_weapon_wants_repairing_not_maintaining() {
        let (mut world, gun, _) = fixture(40.0, 1);
        world.add_component(gun, (PropObjState(ObjectState::Broken),));
        assert_eq!(
            maintenance_outcome(&world, Some(gun)),
            MaintenanceOutcome::Broken
        );
    }

    /// The broken check comes first: an unskilled player working on a broken
    /// weapon is told to repair it, not to train.
    #[test]
    fn a_broken_weapon_reports_broken_even_below_the_skill_requirement() {
        let (mut world, gun, _) = fixture(40.0, 0);
        world.add_component(gun, (PropObjState(ObjectState::Broken),));
        assert_eq!(
            maintenance_outcome(&world, Some(gun)),
            MaintenanceOutcome::Broken
        );
    }

    #[test]
    fn maintaining_below_the_authored_skill_requirement_refuses() {
        let (world, gun, _) = fixture(40.0, 0);
        assert_eq!(
            maintenance_outcome(&world, Some(gun)),
            MaintenanceOutcome::SkillRequired { level: 1 }
        );
    }

    /// A weapon that authors no requirement still needs Maintain 1: the tool
    /// restores ten points a level, so a level-zero use would spend it for
    /// nothing at all.
    #[test]
    fn maintaining_at_skill_zero_is_always_refused() {
        let (mut world, _, _) = fixture(40.0, 0);
        let gun = world.add_entity((PropGunState {
            ammo: 1,
            condition: 40.0,
            setting: 0,
            modification: 0,
            silence_value: 0.0,
        },));

        assert_eq!(
            maintenance_outcome(&world, Some(gun)),
            MaintenanceOutcome::SkillRequired { level: 1 },
            "a zero-point restore would consume the tool and do nothing"
        );
    }

    /// Using a carried item: a weapon wields, a tool works on the wielded
    /// weapon, and anything else keeps its plain Frob.
    #[test]
    fn using_a_tool_with_nothing_wielded_refuses_instead_of_going_nowhere() {
        let (world, _, tool) = fixture(40.0, 1);

        let effect = use_carried_item(&world, tool);
        assert!(
            matches!(&effect, Effect::ShowMessage { text }
                if text == "Drag tool to ranged weapon to use."),
            "got {effect:?}"
        );
    }

    #[test]
    fn using_an_ordinary_item_still_frobs_it() {
        let (mut world, _, _) = fixture(40.0, 1);
        let hypo = world.add_entity((PropObjState(ObjectState::Normal),));

        assert!(
            matches!(
                use_carried_item(&world, hypo),
                Effect::Send {
                    msg: crate::scripts::Message {
                        payload: crate::scripts::MessagePayload::Frob,
                        to,
                    }
                } if to == hypo
            ),
            "an item that is neither a weapon nor a tool keeps its own Frob"
        );
    }

    #[test]
    fn a_weapon_already_in_good_condition_refuses() {
        let (world, gun, _) = fixture(100.0, 1);
        assert_eq!(
            maintenance_outcome(&world, Some(gun)),
            MaintenanceOutcome::AlreadyGood
        );
    }

    #[test]
    fn a_successful_use_restores_the_weapon_and_destroys_a_single_tool() {
        let (world, gun, tool) = fixture(40.0, 1);
        let effects = effects(apply(&world, tool, Some(gun)));

        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::AdjustWeaponCondition { entity_id, delta }
                if *entity_id == gun && *delta == 10.0
        )));
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::DestroyEntity { entity_id } if *entity_id == tool))
        );
    }

    /// The tool stacks, so a stack of more than one loses a unit instead of
    /// being destroyed outright.
    #[test]
    fn a_stack_of_tools_loses_one_unit_per_use() {
        let (mut world, gun, tool) = fixture(40.0, 1);
        world.add_component(tool, (PropStackCount(3),));
        let effects = effects(apply(&world, tool, Some(gun)));

        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::AdjustStackCount { entity_id, delta }
                if *entity_id == tool && *delta == -1
        )));
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, Effect::DestroyEntity { .. }))
        );
    }

    /// Every refusal keeps the tool: it is only spent by work actually done.
    #[test]
    fn a_refused_use_only_posts_a_message_and_keeps_the_tool() {
        for (condition, maintain, expected) in [
            (100.0, 1, "Weapon already in good condition."),
            (40.0, 0, "Maintaining this weapon requires a skill of 1."),
        ] {
            let (world, gun, tool) = fixture(condition, maintain);
            let effect = apply(&world, tool, Some(gun));
            assert!(
                matches!(&effect, Effect::ShowMessage { text } if text == expected),
                "got {effect:?}"
            );
        }
    }
}
