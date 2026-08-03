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

use cgmath::Vector2;
use dark::properties::{ObjectState, PropHackDiff, PropObjState, PropScripts};
use engine::audio::AudioHandle;
use shipyard::{EntityId, Get, View, World};

use crate::gui::{Gui, GuiComponent, GuiConfig, GuiCursor};
use crate::scripts::Effect;

use super::container::{ContainerGui, ContainerGuiMsg, ContainerGuiState};
use super::keypad::{HackOutcomeEffects, HackState, KeyPadMsg, draw_hack_board, handle_hack_msg};

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

fn object_state(world: &World, entity_id: EntityId) -> ObjectState {
    world
        .borrow::<View<PropObjState>>()
        .ok()
        .and_then(|states| states.get(entity_id).ok().map(|state| state.0))
        .unwrap_or(ObjectState::Normal)
}

fn hack_diff(world: &World, entity_id: EntityId) -> Option<PropHackDiff> {
    world
        .borrow::<View<PropHackDiff>>()
        .ok()
        .and_then(|diffs| diffs.get(entity_id).ok().copied())
}

/// A critical failure ruined the crate: its loot is gone and it never opens
/// again.
fn is_ruined(world: &World, entity_id: EntityId) -> bool {
    matches!(
        object_state(world, entity_id),
        ObjectState::Broken | ObjectState::Destroyed
    )
}

/// Whether the crate's contents are reachable. A hacked crate is open; so is
/// a crate an author left unlocked (no `Locked` state), which is then just a
/// plain container.
fn is_open(world: &World, entity_id: EntityId) -> bool {
    !matches!(
        object_state(world, entity_id),
        ObjectState::Locked | ObjectState::Broken | ObjectState::Destroyed
    )
}

/// Whether the crate can still be hacked: still locked, not ruined, and with
/// an authored difficulty to hack against.
fn can_hack(world: &World, entity_id: EntityId) -> bool {
    !is_open(world, entity_id)
        && !is_ruined(world, entity_id)
        && hack_diff(world, entity_id).is_some()
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
    world
        .borrow::<View<PropScripts>>()
        .ok()
        .and_then(|scripts| scripts.get(entity_id).ok().map(|s| s.scripts.clone()))
        .is_some_and(|scripts| {
            scripts
                .iter()
                .any(|script| script.eq_ignore_ascii_case(FREE_HACK_SCRIPT))
        })
}

impl Gui<HackableCrateState, HackableCrateMsg> for HackableCrateGui {
    fn get_components(
        &self,
        cursor: &Option<GuiCursor>,
        entity_id: EntityId,
        world: &World,
        state: &HackableCrateState,
    ) -> Vec<GuiComponent<HackableCrateMsg>> {
        // An open crate is an ordinary loot panel. A still-locked (or
        // just-ruined) one shows the HRM board - a ruined crate keeps it open
        // long enough to render the LOSEH result of the hack that broke it.
        if is_open(world, entity_id) {
            return self
                .loot
                .get_components(cursor, entity_id, world, &state.loot)
                .into_iter()
                .map(|component| component.map(HackableCrateMsg::Loot))
                .collect();
        }

        match hack_diff(world, entity_id) {
            Some(diff) => draw_hack_board(&state.hack, diff, HackableCrateMsg::Hack),
            // No authored difficulty to hack against: fall back to the plain
            // container behavior rather than a dead panel.
            None => self
                .loot
                .get_components(cursor, entity_id, world, &state.loot)
                .into_iter()
                .map(|component| component.map(HackableCrateMsg::Loot))
                .collect(),
        }
    }

    fn get_config(&self) -> GuiConfig {
        // The HRM board and the loot panel share the retail 188x296 MFD
        // canvas, so one config covers both faces of the crate.
        GuiConfig {
            world_offset: self.loot.get_config().world_offset,
            screen_size_in_pixels: Vector2::new(188.0, 296.0),
        }
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
                let Some(diff) = hack_diff(world, entity_id) else {
                    return (state.clone(), Effect::NoEffect);
                };
                // An already-open or ruined crate has nothing left to hack;
                // ignore stale board input against it.
                if !can_hack(world, entity_id) {
                    return (state.clone(), Effect::NoEffect);
                }
                let (hack, effect) = handle_hack_msg(
                    entity_id,
                    world,
                    &state.hack,
                    hack_msg,
                    diff,
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
    /// consumed. Anything else (and a pick offered to an already-open or
    /// ruined crate) falls through to the ordinary deposit path.
    fn on_provide_for_consumption(
        &self,
        entity_id: EntityId,
        world: &World,
        provided_entity_id: EntityId,
    ) -> Option<Effect> {
        if !can_hack(world, entity_id) || !is_free_hack_tool(world, provided_entity_id) {
            return None;
        }
        Some(Effect::combine(vec![
            crate_hack_success(entity_id, world),
            Effect::DestroyEntity {
                entity_id: provided_entity_id,
            },
            Effect::PlaySound {
                handle: AudioHandle::new(),
                name: "hack_success".to_owned(),
            },
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripts::MessagePayload;
    use dark::properties::{Link, Links, PropObjIcon, ToLink, WrappedEntityId};

    /// The shipped hydro1 crate 325: locked, authored HackDiff, one Small HE
    /// Clip on a `Contains` link.
    fn crate_world() -> (World, EntityId, EntityId) {
        let mut world = World::new();
        let clip = world.add_entity(PropObjIcon("icn_bull".to_owned()));
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
        (world, security_crate, clip)
    }

    fn ice_pick(world: &mut World) -> EntityId {
        world.add_entity(PropScripts {
            scripts: vec!["FreeHack".to_owned()],
            inherits: false,
        })
    }

    /// Flatten a returned effect into its leaves, so an assertion can look
    /// for one specific effect regardless of how it was combined.
    fn flatten(effect: Effect) -> Vec<Effect> {
        match effect {
            Effect::Multiple(effects) | Effect::Combined { effects } => {
                effects.into_iter().flat_map(flatten).collect()
            }
            other => vec![other],
        }
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

    /// Winning the board is what opens the crate: the success outcome is a
    /// persistent `Hacked` state, and a critical failure ruins it (the
    /// archetype's own HackText: "Critical failure destroys it").
    #[test]
    fn hack_outcomes_set_the_persistent_crate_state() {
        let (world, security_crate, _clip) = crate_world();

        assert!(
            matches!(
                crate_hack_success(security_crate, &world),
                Effect::SetObjectState {
                    entity_id,
                    state: ObjectState::Hacked,
                } if entity_id == security_crate
            ),
            "a won hack should persistently open the crate"
        );
        assert!(
            matches!(
                crate_hack_critical_failure(security_crate, &world),
                Effect::SetObjectState {
                    entity_id,
                    state: ObjectState::Broken,
                } if entity_id == security_crate
            ),
            "a critical failure should persistently ruin the crate"
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

        let effects = flatten(effect);
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

    /// Only an authored `FreeHack` tool opens a crate; an ordinary item
    /// offered to it still takes the normal deposit path.
    #[test]
    fn an_ordinary_item_is_not_a_free_hack() {
        let (mut world, security_crate, _clip) = crate_world();
        let wrench = world.add_entity(PropObjIcon("icn_wrench".to_owned()));
        let gui = HackableCrateGui::new();

        assert!(
            gui.on_provide_for_consumption(security_crate, &world, wrench)
                .is_none(),
            "a non-tool item must fall through to the ordinary deposit path"
        );
    }

    /// A second pick is not wasted on an already-open crate, and no pick can
    /// resurrect one a critical failure ruined.
    #[test]
    fn ice_pick_is_not_consumed_by_an_open_or_ruined_crate() {
        let (mut world, security_crate, _clip) = crate_world();
        let pick = ice_pick(&mut world);
        let gui = HackableCrateGui::new();

        for state in [ObjectState::Hacked, ObjectState::Broken] {
            world.add_component(security_crate, PropObjState(state));
            assert!(
                gui.on_provide_for_consumption(security_crate, &world, pick)
                    .is_none(),
                "an ICE Pick must not be spent on a {state:?} crate"
            );
        }
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
        let effects = flatten(effect);
        assert!(
            effects.iter().any(
                |effect| matches!(effect, Effect::OpenPanel { entity } if *entity == security_crate)
            ),
            "frobbing a security crate should open its MFD panel, got {effects:?}"
        );
    }
}
