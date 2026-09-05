//! Security crate panel (`HackableCrate`).
//!
//! The shipped crate archetype (shock2.gam template -1886, "Hackable Crate")
//! is authored as a locked container: `P$ObjState(Locked)`, a `P$HackDiff`
//! (success chance / critical chance / nanite cost), the `P$HackText`
//! `"Hack to open crate.  Critical failure destroys it."`, and its loot on
//! outgoing `Contains` links. Its `P$Scripts` is `["HackableCrate"]` with
//! `inherits: false`, so it deliberately does *not* run the `containerscript`
//! it would otherwise inherit from `Usable Containers` (-118) - the crate
//! script itself owns both halves of the object's life.
//!
//! So this panel is exactly that pair, keyed off the persistent `ObjState`:
//!
//! * **Locked** - the shared retail HRM board (`keypad::draw_hack_board` /
//!   [`handle_hack_msg`], the same board the keypads and the replicator use):
//!   START charges the authored nanite cost, hack skill + cyber affinity set
//!   the per-node odds and mine count, three connected nodes win.
//! * **Hacked** - an ordinary loot panel ([`ContainerGui::loot_container`]),
//!   so the contents come out through the normal container MFD.
//! * **Broken** - a critical failure ruined the crate; its contents are gone
//!   for good and it no longer opens ("Critical failure destroys it").
//!
//! An **ICE Pick** (gamesys -73, `P$Scripts ["FreeHack"]`, `tool_action:
//! SCRIPT`) opens a crate outright and is consumed doing it. It arrives on the
//! port's tool channel - `MessagePayload::ProvideForConsumption`, sent when a
//! held item is released onto a target - via [`Gui::on_provide_for_consumption`],
//! so the pick opens the crate instead of being deposited into it.

use dark::properties::ObjectState;
use engine::audio::AudioHandle;
use shipyard::{EntityId, World};

use crate::gui::{Gui, GuiComponent, GuiConfig, GuiCursor};
use crate::scripts::Effect;

use super::container::{ContainerGui, ContainerGuiMsg, ContainerGuiState};
use super::keypad::{
    HackOutcomeEffects, HackPhase, HackState, HrmMode, KeyPadMsg, draw_hack_board, hack_diff,
    handle_hack_msg, object_state,
};

/// The ICE Pick's authored script name (`P$Scripts` on gamesys template -73).
/// Identifying the tool by its authored script - not by a hardcoded template
/// id - keeps any object the data marks as a free hack working.
const FREE_HACK_SCRIPT: &str = "FreeHack";

pub struct HackableCrateGui {
    loot: ContainerGui,
}

impl HackableCrateGui {
    pub fn new() -> HackableCrateGui {
        HackableCrateGui {
            loot: ContainerGui::loot_container(),
        }
    }
}

impl Default for HackableCrateGui {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, Default)]
pub struct HackableCrateState {
    hack: HackState,
    loot: ContainerGuiState,
}

#[derive(Clone)]
pub enum HackableCrateMsg {
    Hack(KeyPadMsg),
    Loot(ContainerGuiMsg),
}

/// A critical failure ruined the crate: its loot is gone and it never opens
/// again.
fn is_ruined(world: &World, entity_id: EntityId) -> bool {
    matches!(
        object_state(world, entity_id),
        ObjectState::Broken | ObjectState::Destroyed
    )
}

/// Whether the crate is still sealed behind a hack: authored `Locked`, with an
/// authored difficulty to hack against. A `Locked` crate with no `P$HackDiff`
/// could never be opened by anything, so it is not treated as sealed.
fn can_hack(world: &World, entity_id: EntityId) -> bool {
    object_state(world, entity_id) == ObjectState::Locked && hack_diff(world, entity_id).is_some()
}

/// Whether the crate's contents are reachable. A hacked crate is open; so is a
/// crate an author left unlocked, which is then just a plain container. This is
/// the single predicate that drawing, input handling, and tool use all agree
/// on, so the panel can never show loot it refuses to hand over.
fn is_open(world: &World, entity_id: EntityId) -> bool {
    !is_ruined(world, entity_id) && !can_hack(world, entity_id)
}

fn crate_hack_success(entity_id: EntityId, _world: &World) -> Effect {
    Effect::SetObjectState {
        entity_id,
        state: ObjectState::Hacked,
    }
}

/// "Critical failure destroys it" (the archetype's own `P$HackText`): the
/// crate is ruined for the rest of the game and its loot is unrecoverable.
fn crate_hack_critical_failure(entity_id: EntityId, _world: &World) -> Effect {
    Effect::SetObjectState {
        entity_id,
        state: ObjectState::Broken,
    }
}

/// Whether `entity_id` is an ICE Pick - an object the data marks with the
/// `FreeHack` script.
fn is_free_hack_tool(world: &World, entity_id: EntityId) -> bool {
    crate::scripts::script_util::entity_has_script(world, entity_id, FREE_HACK_SCRIPT)
}

impl Gui<HackableCrateState, HackableCrateMsg> for HackableCrateGui {
    fn get_components(
        &self,
        cursor: &Option<GuiCursor>,
        entity_id: EntityId,
        world: &World,
        state: &HackableCrateState,
    ) -> Vec<GuiComponent<HackableCrateMsg>> {
        // An open crate is an ordinary loot panel; a sealed one is the board.
        if is_open(world, entity_id) {
            return self
                .loot
                .get_components(cursor, entity_id, world, &state.loot)
                .into_iter()
                .map(|component| component.map(HackableCrateMsg::Loot))
                .collect();
        }

        let Some(diff) = hack_diff(world, entity_id) else {
            // Ruined with no authored difficulty: there is nothing to present
            // and nothing to hack. Unreachable with shipped data (every crate
            // inherits HackDiff from -1886).
            return Vec::new();
        };

        // A ruined crate shows the terminal failure face. Drive it from the
        // durable `ObjState` rather than the in-memory hack phase, so the
        // "destroyed" feedback survives a save/load (which resets the phase)
        // instead of redrawing a live-looking START the crate would refuse.
        let hack = if is_ruined(world, entity_id) {
            HackState {
                phase: HackPhase::Lost,
                ..state.hack.clone()
            }
        } else {
            state.hack.clone()
        };
        draw_hack_board(&hack, diff, HrmMode::Hack, HackableCrateMsg::Hack)
    }

    fn get_config(&self) -> GuiConfig {
        // The HRM board and the loot panel share the retail 188x296 MFD
        // canvas, so the loot panel's own config covers both faces of the
        // crate - and stays in step with it if that panel ever changes.
        self.loot.get_config()
    }

    /// A crate ruined by a critical failure is finished: frobbing it does
    /// nothing at all.
    fn opens_on_frob(&self, entity_id: EntityId, world: &World) -> bool {
        !is_ruined(world, entity_id)
    }

    fn handle_msg(
        &self,
        entity_id: EntityId,
        world: &World,
        state: &HackableCrateState,
        msg: &HackableCrateMsg,
    ) -> (HackableCrateState, Effect) {
        match msg {
            HackableCrateMsg::Hack(hack_msg) => {
                // An already-open or ruined crate has nothing left to hack;
                // ignore stale board input against it.
                if !can_hack(world, entity_id) {
                    return (state.clone(), Effect::NoEffect);
                }
                let Some(diff) = hack_diff(world, entity_id) else {
                    return (state.clone(), Effect::NoEffect);
                };
                let (hack, effect) = handle_hack_msg(
                    entity_id,
                    world,
                    &state.hack,
                    hack_msg,
                    diff,
                    HrmMode::Hack,
                    HackOutcomeEffects {
                        success: crate_hack_success,
                        critical_failure: crate_hack_critical_failure,
                    },
                );
                (
                    HackableCrateState {
                        hack,
                        ..state.clone()
                    },
                    effect,
                )
            }
            // Loot input only counts once the crate is actually open.
            HackableCrateMsg::Loot(loot_msg) => {
                if !is_open(world, entity_id) {
                    return (state.clone(), Effect::NoEffect);
                }
                let (loot, effect) = self
                    .loot
                    .handle_msg(entity_id, world, &state.loot, loot_msg);
                (
                    HackableCrateState {
                        loot,
                        ..state.clone()
                    },
                    effect,
                )
            }
        }
    }

    /// An ICE Pick applied to a still-hackable crate opens it outright and is
    /// consumed. Any other item offered to a *sealed* crate is refused (a
    /// sealed crate is not a container - depositing there would lose the item
    /// permanently); an open crate takes the ordinary deposit path.
    fn on_provide_for_consumption(
        &self,
        entity_id: EntityId,
        world: &World,
        provided_entity_id: EntityId,
    ) -> Option<Effect> {
        if can_hack(world, entity_id) && is_free_hack_tool(world, provided_entity_id) {
            return Some(Effect::combine(vec![
                crate_hack_success(entity_id, world),
                Effect::DestroyEntity {
                    entity_id: provided_entity_id,
                },
                Effect::PlaySound {
                    handle: AudioHandle::new(),
                    source: Some(entity_id),
                    name: "hack_success".to_owned(),
                    spatial: false,
                },
            ]));
        }
        // A sealed crate - still locked, or ruined - is not a container.
        // Refuse the offer outright rather than letting the default deposit
        // path swallow the item into a `Contains` link nothing can ever reach
        // again. An open crate falls through and accepts the deposit normally.
        (!is_open(world, entity_id)).then_some(Effect::NoEffect)
    }
}

#[cfg(test)]
mod tests {
    use super::super::keypad::{HackNode, board_index};
    use super::*;
    use crate::scripts::MessagePayload;
    use cgmath::{Quaternion, vec3};
    use dark::properties::PropScripts;
    use dark::properties::{
        FrobFlag, Link, Links, PropFrobInfo, PropObjIcon, ToLink, WrappedEntityId,
    };
    use dark::properties::{PropHackDiff, PropObjState};

    /// The shipped hydro1 crate 325: locked, authored HackDiff, one Small HE
    /// Clip on a `Contains` link - plus the player backpack the loot path
    /// transfers into, so taking an item can be asserted end to end.
    fn crate_world() -> (World, EntityId, EntityId) {
        let mut world = World::new();
        let clip = world.add_entity((
            PropObjIcon("icn_bull".to_owned()),
            PropFrobInfo {
                world_action: FrobFlag::MOVE,
                inventory_action: FrobFlag::empty(),
                tool_action: FrobFlag::empty(),
            },
        ));
        let security_crate = world.add_entity((
            PropObjState(ObjectState::Locked),
            PropHackDiff {
                success_chance: -10,
                critical_chance: 5,
                cost: 5.0,
            },
            Links {
                to_links: vec![ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(clip)),
                    link: Link::Contains(0),
                }],
            },
        ));
        let player = world.add_entity(());
        let inventory = world.add_entity(Links::empty());
        world.add_unique(crate::mission::PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory,
        });
        (world, security_crate, clip)
    }

    fn ice_pick(world: &mut World) -> EntityId {
        world.add_entity(PropScripts {
            scripts: vec!["FreeHack".to_owned()],
            inherits: false,
        })
    }

    fn has_texture<T: Clone>(components: &[GuiComponent<T>], texture: &str) -> bool {
        components.iter().any(|component| {
            matches!(component, GuiComponent::Image { texture: t, .. } if t.eq_ignore_ascii_case(texture))
        })
    }

    fn button_for<T: Clone>(components: &[GuiComponent<T>], entity: EntityId) -> bool {
        components
            .iter()
            .any(|component| matches!(component, GuiComponent::Button { entity: e, .. } if *e == Some(entity)))
    }

    /// A locked crate presents the HRM board, NOT its contents - the loot is
    /// genuinely sealed behind the hack.
    #[test]
    fn locked_crate_shows_the_hack_board_and_hides_its_loot() {
        let (world, security_crate, clip) = crate_world();
        let gui = HackableCrateGui::new();
        let components = gui.get_components(
            &None,
            security_crate,
            &world,
            &HackableCrateState::default(),
        );

        assert!(
            has_texture(&components, "hack.pcx"),
            "a locked crate should draw the shared retail HRM board"
        );
        assert!(
            !button_for(&components, clip),
            "a locked crate must not expose its contents"
        );
    }

    /// Once hacked, the crate is an ordinary loot container: the contained
    /// clip gets its own clickable slot.
    #[test]
    fn hacked_crate_shows_its_contents_as_a_loot_panel() {
        let (mut world, security_crate, clip) = crate_world();
        world.add_component(security_crate, PropObjState(ObjectState::Hacked));

        let gui = HackableCrateGui::new();
        let components = gui.get_components(
            &None,
            security_crate,
            &world,
            &HackableCrateState::default(),
        );

        assert!(
            has_texture(&components, "contain.pcx"),
            "a hacked crate should draw the loot panel backdrop"
        );
        assert!(
            button_for(&components, clip),
            "a hacked crate should expose its contained clip as a loot slot"
        );
    }

    /// A live board with two nodes already lit, so lighting `target` decides
    /// the round. `success_chance` drives the per-node roll (the no-HRM-params
    /// fallback clamps it to 0..=85, so 0 always fails and 85 nearly always
    /// succeeds).
    fn board_one_node_from_the_end(
        target_is_mine: bool,
        rng_state: u64,
    ) -> (HackState, PropHackDiff) {
        let mut nodes = super::super::keypad::base_hack_board();
        nodes[board_index(2, 0)] = HackNode::Lit;
        nodes[board_index(3, 0)] = HackNode::Lit;
        nodes[board_index(4, 0)] = if target_is_mine {
            HackNode::Mine
        } else {
            HackNode::Free
        };
        let diff = PropHackDiff {
            success_chance: if target_is_mine { 0 } else { 85 },
            critical_chance: 0,
            cost: 5.0,
        };
        (
            HackState {
                phase: HackPhase::Playing,
                nodes,
                rng_state,
            },
            diff,
        )
    }

    /// Playing the winning node through the real `handle_msg` path opens the
    /// crate persistently. This drives the actual `HackOutcomeEffects` wiring,
    /// so swapping the success and critical-failure handlers fails it.
    #[test]
    fn winning_the_board_persistently_opens_the_crate() {
        let (mut world, security_crate, _clip) = crate_world();
        let gui = HackableCrateGui::new();

        let mut wins = 0;
        for rng_state in 1..=32u64 {
            let (hack, diff) = board_one_node_from_the_end(false, rng_state);
            world.add_component(security_crate, diff);
            let (_after, effect) = gui.handle_msg(
                security_crate,
                &world,
                &HackableCrateState {
                    hack,
                    ..HackableCrateState::default()
                },
                &HackableCrateMsg::Hack(KeyPadMsg::PlayNode { x: 4, y: 0 }),
            );
            let effects = Effect::flatten(vec![effect]);
            assert!(
                !effects.iter().any(|effect| matches!(
                    effect,
                    Effect::SetObjectState {
                        state: ObjectState::Broken,
                        ..
                    }
                )),
                "a mine-free board must never ruin the crate (seed {rng_state})"
            );
            if effects.iter().any(|effect| matches!(
                effect,
                Effect::SetObjectState { entity_id, state: ObjectState::Hacked } if *entity_id == security_crate
            )) {
                wins += 1;
            }
        }
        assert!(
            wins > 0,
            "connecting the third node should open the crate on at least one seed"
        );
    }

    /// Hitting a mine ruins the crate for good - the archetype's own HackText:
    /// "Critical failure destroys it". With a 0% node chance every roll fails,
    /// so this is deterministic.
    #[test]
    fn hitting_a_mine_persistently_ruins_the_crate() {
        let (mut world, security_crate, _clip) = crate_world();
        let gui = HackableCrateGui::new();
        let (hack, diff) = board_one_node_from_the_end(true, 7);
        world.add_component(security_crate, diff);

        let (_after, effect) = gui.handle_msg(
            security_crate,
            &world,
            &HackableCrateState {
                hack,
                ..HackableCrateState::default()
            },
            &HackableCrateMsg::Hack(KeyPadMsg::PlayNode { x: 4, y: 0 }),
        );
        let effects = Effect::flatten(vec![effect]);
        assert!(
            effects.iter().any(|effect| matches!(
                effect,
                Effect::SetObjectState { entity_id, state: ObjectState::Broken } if *entity_id == security_crate
            )),
            "a critical failure should persistently ruin the crate: {effects:?}"
        );
    }

    /// A crate ruined by a critical failure never opens again.
    #[test]
    fn ruined_crate_does_not_open_on_frob() {
        let (mut world, security_crate, _clip) = crate_world();
        let gui = HackableCrateGui::new();
        assert!(
            gui.opens_on_frob(security_crate, &world),
            "a locked crate should still open its hack panel"
        );

        world.add_component(security_crate, PropObjState(ObjectState::Broken));
        assert!(
            !gui.opens_on_frob(security_crate, &world),
            "a crate destroyed by a critical failure must not reopen"
        );
    }

    /// The nanite charge comes from the authored `P$HackDiff` cost, through
    /// the same shared board the keypads and replicator use: with no nanites
    /// carried, START cannot be paid and no board is dealt.
    #[test]
    fn starting_a_hack_without_nanites_does_not_deal_a_board() {
        let (world, security_crate, _clip) = crate_world();
        let gui = HackableCrateGui::new();

        let (after, _effect) = gui.handle_msg(
            security_crate,
            &world,
            &HackableCrateState::default(),
            &HackableCrateMsg::Hack(KeyPadMsg::StartHack),
        );
        assert_eq!(
            after.hack.phase,
            super::super::keypad::HackPhase::InsufficientNanites,
            "an unpayable hack should report the shortfall rather than start"
        );
    }

    /// An ICE Pick applied to a locked crate opens it outright AND is
    /// consumed - it must not be deposited into the crate as loot.
    #[test]
    fn ice_pick_opens_the_crate_and_is_consumed() {
        let (mut world, security_crate, _clip) = crate_world();
        let pick = ice_pick(&mut world);
        let gui = HackableCrateGui::new();

        let effect = gui
            .on_provide_for_consumption(security_crate, &world, pick)
            .expect("an ICE Pick should be claimed as a tool, not deposited");

        let effects = Effect::flatten(vec![effect]);
        assert!(
            effects.iter().any(|effect| matches!(
                effect,
                Effect::SetObjectState { entity_id, state: ObjectState::Hacked } if *entity_id == security_crate
            )),
            "an ICE Pick should open the crate outright: {effects:?}"
        );
        assert!(
            effects.iter().any(|effect| matches!(
                effect,
                Effect::DestroyEntity { entity_id } if *entity_id == pick
            )),
            "the ICE Pick should be consumed by the use: {effects:?}"
        );
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::DropEntityInfo { .. })),
            "the pick must not also be deposited into the crate: {effects:?}"
        );
    }

    /// Only an authored `FreeHack` tool opens a crate: any other item offered
    /// to a locked one must leave it locked and must not be destroyed.
    #[test]
    fn an_ordinary_item_is_not_a_free_hack() {
        let (mut world, security_crate, _clip) = crate_world();
        let wrench = world.add_entity(PropObjIcon("icn_wrench".to_owned()));
        let gui = HackableCrateGui::new();

        let effects = Effect::flatten(vec![
            gui.on_provide_for_consumption(security_crate, &world, wrench)
                .unwrap_or(Effect::NoEffect),
        ]);
        assert!(
            effects.is_empty(),
            "a non-tool item must neither open the crate nor be consumed: {effects:?}"
        );
    }

    /// A second pick is not wasted on an already-open crate, and no pick can
    /// resurrect one a critical failure ruined. An open crate is a normal
    /// container, so the pick falls through to the ordinary deposit; a ruined
    /// crate is sealed, so the offer is refused outright.
    #[test]
    fn ice_pick_is_not_consumed_by_an_open_or_ruined_crate() {
        let (mut world, security_crate, _clip) = crate_world();
        let pick = ice_pick(&mut world);
        let gui = HackableCrateGui::new();

        world.add_component(security_crate, PropObjState(ObjectState::Hacked));
        assert!(
            gui.on_provide_for_consumption(security_crate, &world, pick)
                .is_none(),
            "an open crate is a container: the pick is deposited, not spent"
        );

        world.add_component(security_crate, PropObjState(ObjectState::Broken));
        assert!(
            matches!(
                gui.on_provide_for_consumption(security_crate, &world, pick),
                Some(Effect::NoEffect)
            ),
            "a ruined crate must refuse the pick outright, not consume it"
        );
    }

    /// A sealed crate is NOT a container: an item offered to a locked or
    /// ruined crate must be refused, never swallowed into a `Contains` link
    /// the player could never reach again (a quest item dropped on a ruined
    /// crate would otherwise be lost for good).
    #[test]
    fn a_sealed_crate_refuses_a_deposit_instead_of_swallowing_it() {
        let (mut world, security_crate, _clip) = crate_world();
        let keycard = world.add_entity(PropObjIcon("icn_card".to_owned()));
        let gui = HackableCrateGui::new();

        for state in [ObjectState::Locked, ObjectState::Broken] {
            world.add_component(security_crate, PropObjState(state));
            assert!(
                matches!(
                    gui.on_provide_for_consumption(security_crate, &world, keycard),
                    Some(Effect::NoEffect)
                ),
                "a {state:?} crate must refuse a deposit, not swallow the item"
            );
        }

        // ...while an opened crate accepts deposits like any container.
        world.add_component(security_crate, PropObjState(ObjectState::Hacked));
        assert!(
            gui.on_provide_for_consumption(security_crate, &world, keycard)
                .is_none(),
            "an opened crate should take the ordinary deposit path"
        );
    }

    /// A `Locked` crate with no authored `P$HackDiff` can never be hacked by
    /// anything, so it must behave as a plain container rather than show a
    /// panel full of loot it refuses to hand over.
    #[test]
    fn a_locked_crate_without_a_hack_difficulty_is_a_plain_container() {
        let (mut world, security_crate, clip) = crate_world();
        world.delete_component::<PropHackDiff>(security_crate);
        let gui = HackableCrateGui::new();

        let components = gui.get_components(
            &None,
            security_crate,
            &world,
            &HackableCrateState::default(),
        );
        assert!(
            button_for(&components, clip),
            "a crate with nothing to hack should present its contents"
        );

        let (_after, effect) = gui.handle_msg(
            security_crate,
            &world,
            &HackableCrateState::default(),
            &HackableCrateMsg::Loot(ContainerGuiMsg::Take(clip)),
        );
        assert!(
            matches!(effect, Effect::DropEntityInfo { dropped_entity_id, .. } if dropped_entity_id == clip),
            "and it should actually hand them over, got {effect:?}"
        );
    }

    /// A ruined crate's terminal face is driven by the durable `ObjState`, not
    /// the in-memory hack phase - so after a save/load (which resets the
    /// phase) it still reads as destroyed instead of offering a live START the
    /// crate would silently refuse.
    #[test]
    fn a_ruined_crate_shows_its_failure_face_after_state_is_reset() {
        let (mut world, security_crate, _clip) = crate_world();
        world.add_component(security_crate, PropObjState(ObjectState::Broken));
        let gui = HackableCrateGui::new();

        // Default state = the freshly-loaded panel (HackPhase::Unpaid).
        let components = gui.get_components(
            &None,
            security_crate,
            &world,
            &HackableCrateState::default(),
        );
        assert!(
            has_texture(&components, "loseh.pcx"),
            "a ruined crate should still read as destroyed"
        );
        assert!(
            !components.iter().any(|component| matches!(
                component,
                GuiComponent::Button { label: Some(label), .. } if label == "start-hack"
            )),
            "and must not offer a START it would refuse"
        );
    }

    /// Board input against an already-open crate is inert (no second charge,
    /// no state churn).
    #[test]
    fn hack_input_against_an_open_crate_is_inert() {
        let (mut world, security_crate, _clip) = crate_world();
        world.add_component(security_crate, PropObjState(ObjectState::Hacked));
        let gui = HackableCrateGui::new();

        let (_after, effect) = gui.handle_msg(
            security_crate,
            &world,
            &HackableCrateState::default(),
            &HackableCrateMsg::Hack(KeyPadMsg::StartHack),
        );
        assert!(
            matches!(effect, Effect::NoEffect),
            "an open crate should ignore board input, got {effect:?}"
        );
    }

    /// Loot input against a still-locked crate is inert - the sealed contents
    /// cannot be taken by a stale click.
    #[test]
    fn loot_input_against_a_locked_crate_is_inert() {
        let (world, security_crate, clip) = crate_world();
        let gui = HackableCrateGui::new();

        let (_after, effect) = gui.handle_msg(
            security_crate,
            &world,
            &HackableCrateState::default(),
            &HackableCrateMsg::Loot(ContainerGuiMsg::Take(clip)),
        );
        assert!(
            matches!(effect, Effect::NoEffect),
            "a locked crate must not yield its loot, got {effect:?}"
        );

        // Control: the identical click transfers once the crate is opened, so
        // the refusal above is the lock and not an unrelated container guard.
        let mut world = world;
        world.add_component(security_crate, PropObjState(ObjectState::Hacked));
        let (_after, effect) = gui.handle_msg(
            security_crate,
            &world,
            &HackableCrateState::default(),
            &HackableCrateMsg::Loot(ContainerGuiMsg::Take(clip)),
        );
        assert!(
            matches!(effect, Effect::DropEntityInfo { dropped_entity_id, .. } if dropped_entity_id == clip),
            "an opened crate should hand the same item over, got {effect:?}"
        );
    }

    /// The whole point of the panel: frobbing a crate must actually route
    /// somewhere. Guard the script wiring so the archetype never silently
    /// falls back to an unimplemented no-op again (#811).
    #[test]
    fn hackable_crate_script_is_wired_to_the_panel() {
        let mut script = crate::scripts::ScriptWorld::create_script("HackableCrate".to_owned());
        let (world, security_crate, _clip) = crate_world();
        let physics = crate::physics::PhysicsWorld::new();

        let effect = script.handle_message(security_crate, &world, &physics, &MessagePayload::Frob);
        let effects = Effect::flatten(vec![effect]);
        assert!(
            effects.iter().any(
                |effect| matches!(effect, Effect::OpenPanel { entity } if *entity == security_crate)
            ),
            "frobbing a security crate should open its MFD panel, got {effects:?}"
        );
    }
}
