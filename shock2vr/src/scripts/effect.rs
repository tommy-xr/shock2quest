use cgmath::{Matrix4, Point3, Quaternion, Vector2, Vector3, Vector4};
use dark::{
    EnvSoundQuery,
    motion::{MotionQueryItem, MotionQuerySelectionStrategy},
    properties::{
        AIAlertLevel, AIMode, KeyCard, ObjectState, PropGunState, PropReplicatorHackedContents,
        QuestBitValue, TeleportSource,
    },
};
use engine::audio::AudioHandle;
use shipyard::EntityId;

use crate::{
    gui::{GuiComponentRenderInfo, GuiHandle},
    mission::entity_creator::CreateEntityOptions,
    vr_config::Handedness,
};

use super::Message;

/// How a level transition initializes the destination player's HP/PSI pools.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerVitalsTransition {
    /// Carry the live pools into the destination. This is the normal behavior
    /// for deck changes, reloads, and station training-year transitions.
    Preserve,
    /// Let the destination derive fresh pools from its player template,
    /// selected career, and persistent traits. Used when character creation
    /// deliberately selects the first career loadout.
    InitializeFromDestination,
}

#[derive(Clone, Debug)]
pub enum GlobalEffect {
    // Save the game state to the given file_name
    Save {
        file_name: String,
    },

    // Load the game state from the given file_naem
    Load {
        file_name: String,
    },

    TransitionLevel {
        level_file: String,
        loc: Option<i32>,
        entities_to_trigger: Vec<String>,
        vitals_transition: PlayerVitalsTransition,
    },

    // Test the reload functionality (as if saving + loading)
    TestReload,

    /// The player died with no reconstruction available: replace the mission
    /// with the game-over screen, which offers the load/quit recovery path.
    GameOver,

    /// Play the retail ending and enter the campaign's terminal state.
    CompleteCampaign,

    // Quit the game (e.g. from the main menu). The runtime is responsible for
    // observing this via `Game::should_quit` and closing its window.
    Quit,
}

#[derive(Clone, Debug)]
pub enum AIPropertyUpdate {
    Alertness {
        level: AIAlertLevel,
        peak: AIAlertLevel,
    },
    Mode {
        mode: AIMode,
    },
    /// Name of the behavior the AI is currently running (debug introspection)
    Behavior {
        name: String,
    },
    /// Horizontal locomotion speed scale (heading-error / arrival
    /// coupling); consumed by the animation velocity write
    LocomotionScale {
        scale: f32,
    },
    /// What the AI knows about its target (last-known position + current
    /// line of sight) - drives non-omniscient chase steering
    TargetAwareness {
        last_known_pos: Vector3<f32>,
        has_line_of_sight: bool,
    },
    /// The AI forgot its target (fully calmed / position consumed by a
    /// search) - chase falls back to the true player position again
    ClearTargetAwareness,
}

#[derive(Clone, Debug)]
pub enum Effect {
    NoEffect,

    AwardXP {
        amount: i32,
    },

    AdjustHitPoints {
        entity_id: EntityId,
        delta: i32,
    },

    /// Adjust an item's stack count (`P$StackCoun`) by `delta`. The caller is
    /// responsible for destroying a stack it exhausts; this effect is used by
    /// authored-currency operations such as the HRM hacking board.
    AdjustStackCount {
        entity_id: EntityId,
        delta: i32,
    },

    /// Adjust a weapon's current clip ammo (`PropGunState.ammo`) by `delta`
    /// (negative to consume). No-op for entities without a gun state.
    AdjustAmmo {
        entity_id: EntityId,
        delta: i32,
    },

    /// Raise an energy weapon's charge to at least its authored capacity.
    /// Applied against the live gun state so duplicate recharge messages in one
    /// tick remain idempotent. No-op for entities without a gun state.
    RechargeAmmo {
        entity_id: EntityId,
        capacity: i32,
    },

    /// Cycle the player's wielded weapon to its next ammo type (the next
    /// `Projectile` link). No-op when no weapon is wielded or it has fewer than
    /// two projectile links, or while its magazine still has loaded rounds.
    CycleAmmo,

    /// Equip a weapon of this gamesys class from the player's carried items.
    /// The handler resolves the live entity through the template hierarchy and
    /// then reuses `GrabEntity`, so no item is spawned and a displaced flat
    /// weapon returns to the backpack.
    EquipCarriedWeapon {
        class_template_id: i32,
    },

    /// Toggle the flat-mode "use" (metagame) mode - cursor-driven UI over the
    /// 3D view, as the original game does on Tab. Currently only tracks the
    /// mode (introspectable via the debug runtime's `GET /v1/ui`); the cursor
    /// and panel presentation land with the flat UI host. No-op in VR.
    /// See `projects/flat-ui.md`.
    ToggleUseMode,

    /// Open the flat-mode MFD panel bound to `entity` - emitted by `GuiScript`
    /// when its entity is frobbed, mirroring the original engine where the
    /// frob script opens the overlay (`cShockGameSrv::Keypad`). Ignored in VR,
    /// where panels are always-on world quads. See `projects/flat-ui.md` §5.2.
    OpenPanel {
        entity: EntityId,
    },

    /// Start or resume research on a carried researchable object.
    BeginResearch {
        entity_id: EntityId,
    },

    /// Offer one carried chemical to the active research project. The handler
    /// consumes it only when it matches the currently authored gate.
    UseResearchChemical {
        entity_id: EntityId,
    },

    /// Record an audio log the player just frobbed into the persistent
    /// collection (`QuestInfo`), resolve its transcript/portrait strings onto
    /// the disc entity (`RuntimePropLogData`) for the reader panel, then retire
    /// the pickup's world presence. Emitted by the log disc's `MediaGui` on
    /// frob. `entity_id` is retained only as reader backing state; `deck`/`log`
    /// key the `level<deck>.str` strings.
    CollectLog {
        entity_id: EntityId,
        deck: u32,
        log: u32,
    },

    /// Play back the most recently collected audio log the player has not read
    /// yet (the original's `play_unread_log`): open the reader bound to that
    /// disc's backing entity, mark it read, and play its `LOG<dd><nn>` audio.
    /// No-op when every collected log has been read. Emitted by
    /// `InputAction::ReadLastUnreadLog`.
    ReadLastUnreadLog,

    /// Dismiss the open MFD panel, if any.
    CloseActivePanel,

    /// Toggle the flat-mode automap panel (the original's BIOFULL MAP button /
    /// `M` key, `kOverlayMap 26`). Opens the wide map MFD bound to the
    /// synthetic map-panel entity, or closes it if it is already open. No-op in
    /// VR and in scenes without a map panel. See `projects/flat-ui-panels.md` §5.
    ToggleMap,

    /// Mark an automap location as explored for the current mission (persisted
    /// in `QuestInfo`, the original's mission-scoped `EXPLORED[64]` file-var).
    /// Emitted by `CoreRoom` when the player enters a room whose room object
    /// carries `PropMapLoc`.
    RevealMapLocation {
        location: i32,
    },

    /// Select the player's next *trained* psi power (advances
    /// `PsiPowerSelection` through the `GlobalPsiPowers` registry, wrapping,
    /// skipping powers not in `PlayerPsiKnownPowers`).
    CyclePsiPower,

    /// Train the player in a psi power (insert its template id into
    /// `PlayerPsiKnownPowers`), making it selectable and castable - for
    /// trainers and debug tooling. No-op if already trained.
    GrantPsiPower {
        template_id: i32,
    },

    /// Deduct psi points from the player's pool (`PropPsiState`), clamped at
    /// zero. Emitted by the psi amp when a power is cast.
    SpendPsiPoints {
        amount: i32,
    },

    /// Set the player's current psi-point pool, clamped to `[0, max]`.
    /// Emitted by authored psi consumables and training-room scripts.
    SetPsiPoints {
        points: i32,
    },

    /// Atomically restore the live player's psi pool and consume one unit of
    /// the originating booster. Applied against live state so simultaneous
    /// uses add independently, while an already-full pool or an already-used
    /// entity remains untouched.
    UsePsiKit {
        entity_id: EntityId,
        amount: i32,
    },

    /// Install a soft on the character sheet and consume the object it came
    /// from. Emitted by `scripts::auto_install_soft` when a soft is frobbed in
    /// the world or taken from a container. The applier does the atomic
    /// compare (installing only ever raises the version) so a redundant soft
    /// is reported rather than downgrading the sheet; either way the object is
    /// consumed - softs never occupy inventory.
    InstallSoftware {
        entity_id: EntityId,
        software: crate::player_stats::Software,
        level: i32,
    },

    /// Activate (or refresh) a sustained psi power on the player for
    /// `duration_secs` (`ActivePsiPowers` unique). Emitted by the psi amp
    /// when a sustained (activation type 1) power is cast; a per-frame tick
    /// in `MissionCore::update` counts the duration down and expires it.
    ActivatePsiPower {
        template_id: i32,
        name: String,
        duration_secs: f32,
    },

    /// Set the psi amp's hold-to-overload meter state
    /// (`RuntimePropPsiCharge`) on the amp entity - drives the HUD meter and
    /// debug introspection while a charge is in progress or flashing its
    /// result.
    SetPsiCharge {
        entity_id: EntityId,
        fraction: f32,
        phase: crate::runtime_props::PsiChargePhase,
    },

    /// Remove the psi amp's charge meter state (charge resolved and the
    /// result flash expired).
    ClearPsiCharge {
        entity_id: EntityId,
    },

    /// Reload the player's wielded weapon from compatible backpack reserve,
    /// up to its magazine capacity (`PropBaseGunDesc.clip`). No-op when no
    /// weapon is wielded, it has no gun state / clip, or no matching reserve is
    /// carried.
    ReloadWeapon,

    ApplyForce {
        entity_id: EntityId,
        force: Vector3<f32>,
    },

    /// A radius stim blast (explosions): entities within `radius` of `center`
    /// receive the stim at `intensity` with linear distance falloff - damage
    /// is resolved per target through its receptrons for `stim_template_id`
    /// (no receptron = no response) - and every dynamic body in range is
    /// shoved outward. Emitted once by `internal_explosion` from the entity's
    /// arSrcDesc data.
    RadiusBlast {
        center: Vector3<f32>,
        radius: f32,
        intensity: f32,
        stim_template_id: i32,
    },

    /// A noise (gunfire, etc.) at `origin`: every creature within `radius`
    /// hears it, escalates alertness, and investigates the source - so
    /// firing a weapon draws nearby AIs even with no line of sight. A plain
    /// Euclidean radius for now (walls don't attenuate it yet).
    RaiseNoise {
        origin: Vector3<f32>,
        radius: f32,
    },

    ChangeModel {
        entity_id: EntityId,
        model_name: String,
    },
    /// Replace the entity's fire-point vhots with those of `model_name`
    /// without changing the rendered model. Used by the VR held-weapon path:
    /// the world model stays rendered (#352), but its mesh has no vhots -
    /// the muzzle points live in the _h viewmodel.
    SetVhotsFromModel {
        entity_id: EntityId,
        model_name: String,
    },
    CreateEntityByTemplateName {
        source_entity_id: EntityId,
        template_name: String,
        position: Point3<f32>,
        orientation: Quaternion<f32>,
        initial_velocity: Vector3<f32>,
    },

    CreateEntity {
        template_id: i32,
        position: Point3<f32>,
        orientation: Quaternion<f32>,
        root_transform: Matrix4<f32>,
        options: CreateEntityOptions,
    },

    /// `TrapSpawn` creation with the Dark ecology bookkeeping that cannot be
    /// expressed until the fresh runtime entity id exists.
    SpawnEcologyEntity {
        template_name: String,
        spawn_point: EntityId,
        ecology_type: Option<i32>,
        goto_player: bool,
    },

    DrawDebugLines {
        lines: Vec<(Point3<f32>, Point3<f32>, Vector4<f32>)>,
    },

    /// Play the flat melee viewmodel's swing animation for `entity_id` (the
    /// wielded melee weapon). Emitted by the melee attack; a no-op in VR.
    FlatMeleeSwing {
        entity_id: EntityId,
    },

    DestroyEntity {
        entity_id: EntityId,
    },
    DropEntityInfo {
        parent_entity_id: EntityId,
        dropped_entity_id: EntityId,
    },
    GrabEntity {
        entity_id: EntityId,
        hand: Handedness,
        current_parent_id: Option<EntityId>,
    },
    SlayEntity {
        entity_id: EntityId,
    },

    /// The death (crumple) animation finished: hand the corpse over to physics
    /// as a ragdoll, optionally seeded with the killing blow (so the corpse
    /// reacts at the struck limb, in the shot's direction). Emitted
    /// unconditionally by the AI on death-animation completion; the handler is
    /// a no-op unless the `ragdoll` experimental flag is on (the animated
    /// corpse entity stays, exactly as before).
    SpawnCorpseRagdoll {
        entity_id: EntityId,
        impact: Option<crate::scripts::DamageImpact>,
    },

    QueueAnimationBySchema {
        // ActorType, MotActorTags get inferred from the entity id
        entity_id: EntityId,
        selection_strategy: MotionQuerySelectionStrategy,
        /// Queries tried in order; the first one that matches a motion wins.
        /// Lets a caller prefer a creature's directly-keyed clips and fall
        /// back to a context-tagged variant only when there are none.
        motion_queries: Vec<Vec<MotionQueryItem>>,
    },

    /// Like `QueueAnimationBySchema`, but interrupts the playing animation
    /// immediately (cross-fading from its current pose) and clears the
    /// queue instead of pushing on top of it. Used for reactions that must
    /// not wait for the in-flight clip, e.g. death.
    PlayAnimationBySchema {
        entity_id: EntityId,
        selection_strategy: MotionQuerySelectionStrategy,
        /// Queries tried in order; the first one that matches a motion wins.
        motion_queries: Vec<Vec<MotionQueryItem>>,
    },

    ReplaceEntity {
        entity_id: EntityId,
        template_id: i32,
    },

    Send {
        msg: Message,
    },
    PlayEmail {
        deck: u32,
        email: u32,
        force: bool,
    },
    PlaySound {
        handle: AudioHandle,
        name: String,
        /// The entity the sound belongs to, when the emitter knows it. Purely
        /// diagnostic (recorded in `audio_log`); `None` for sounds with no
        /// world source, e.g. cutscene schemas.
        source: Option<EntityId>,
        /// Play positionally at `source`'s location (original-engine object
        /// sound behavior). Only for diegetic emitters anchored in the world
        /// (e.g. TrapSound narrations); non-diegetic audio that must stay at
        /// constant volume - audio logs, cutscene narration, UI feedback -
        /// keeps this false even when it has a `source` for attribution.
        spatial: bool,
    },
    PlaySpeech {
        entity_id: EntityId,
        voice_index: usize,
        concept: String,
        tags: Vec<(String, String)>,
    },
    PlayEnvironmentalSound {
        audio_handle: AudioHandle,
        query: EnvSoundQuery,
        position: Vector3<f32>,
    },
    PositionInventory {
        position: Vector3<f32>,
        rotation: Quaternion<f32>,
    },
    StopSound {
        handle: AudioHandle,
    },
    SetPosition {
        entity_id: EntityId,
        position: Vector3<f32>,
    },
    SetRotation {
        entity_id: EntityId,
        rotation: Quaternion<f32>,
    },
    SetPositionRotation {
        entity_id: EntityId,
        position: Vector3<f32>,
        rotation: Quaternion<f32>,
    },
    SetJointTransform {
        entity_id: EntityId,
        joint_id: u32,
        transform: Matrix4<f32>,
    },
    SetPlayerPosition {
        position: Vector3<f32>,
        is_teleport: bool,
        /// What repositioned the player. `ScriptedTrap` arrivals suppress
        /// tripwire ENTER (#515); `Locomotion` (VR teleport) still fires it.
        source: TeleportSource,
    },

    /// Set the player's pawn yaw, used by authored seating/cutscene markers.
    SetPlayerRotation {
        rotation: Quaternion<f32>,
    },

    /// Allow or suppress ordinary player movement and interaction channels.
    /// Discrete recovery actions such as quick-load remain available.
    SetPlayerControlsEnabled {
        enabled: bool,
    },

    /// Full-screen white transition overlay (0.0 = clear, 1.0 = white).
    SetScreenFade {
        alpha: f32,
    },

    /// Remove an entity's `PropTeleported` marker. Emitted by a tripwire once it
    /// has consumed a scripted-teleport arrival, so the ~1s marker can't also
    /// suppress a later walk-in to a *different* nearby tripwire (#515).
    ClearTeleportedMarker {
        entity_id: EntityId,
    },

    /// Set an entity's render alpha (Renderer\Transparency (alpha): 1.0 =
    /// opaque, 0.0 = invisible). Drives holo/ghost fades (Transluce scripts,
    /// CS9 cutscene exhibits).
    SetRenderAlpha {
        entity_id: EntityId,
        alpha: f32,
    },

    /// Show or hide an entity (PropHasRefs) - e.g. apparitions materializing
    /// on ApparBegin and vanishing on ApparEnd.
    SetVisibility {
        entity_id: EntityId,
        visible: bool,
    },

    /// Persistently set Dark's `P$ObjState` on one live object. Replicator HRM
    /// success uses Hacked; a critical failure uses Broken. Because ObjState is
    /// a registered Dark property, mission save/load serializes the result.
    SetObjectState {
        entity_id: EntityId,
        state: ObjectState,
    },

    /// Persistently switch one authored animated light and update the matching
    /// world-representation lightmap layers. `intensity` is normalized against
    /// the light's authored maximum; `inactive` is stored in `P$AnimLight` so
    /// the state survives mission save/load.
    SetAnimatedLight {
        entity_id: EntityId,
        intensity: f32,
        inactive: bool,
    },

    /// Persistently replace a replicator's hacked catalog. Retail's
    /// `PutBombInReplicator` uses this to add the Command-deck objective item;
    /// the registered Dark property makes the change survive save/load.
    SetReplicatorHackedContents {
        entity_id: EntityId,
        contents: PropReplicatorHackedContents,
    },

    /// Persistently set Dark's `P$EcoState` on one live object - emitted by
    /// `scripts::trigger_ecology` for alarm/recovery transitions. Most
    /// ecologies author no explicit EcoState, so this may create the
    /// component. A registered Dark property, so save/load serializes it.
    SetEcologyState {
        entity_id: EntityId,
        state: i32,
    },

    /// Persistently set Dark's `P$Locked` on one live object - the lock state
    /// `script_util::is_entity_locked` consults. Emitted by the in-world lock
    /// traps; see `scripts::trap_lock`.
    SetLocked {
        entity_id: EntityId,
        locked: bool,
    },

    /// Persistently set Dark's `P$TransDoor` motion fields (`state` and
    /// `base_location`) on one live object - emitted by `scripts::std_door` so
    /// door motion survives saves.
    SetTranslatingDoorState {
        entity_id: EntityId,
        state: i32,
        base_location: Vector3<f32>,
    },

    ResetGravity {
        entity_id: EntityId,
    },

    SetGravity {
        entity_id: EntityId,
        gravity_percent: f32,
    },

    SetQuestBit {
        quest_bit_name: String,
        quest_bit_value: QuestBitValue,
    },

    /// Apply the training-tour reward for `(career, year, tour)` to the player's
    /// persistent stats (in `QuestInfo`). Applied at most once per training
    /// year. Emitted by `ChooseMissionScript` on tour completion.
    GrantTourReward {
        career: crate::career::Career,
        year: u32,
        tour: u32,
    },

    /// Buy one trainer upgrade: atomically validate (cost table + caps +
    /// module balance), spend the cyber modules and raise the target stat /
    /// skill / psi tier in the player's persistent stats. Emitted by
    /// `TrainerGui`; validation lives in `scripts::gui::trainer::upgrade_quote`
    /// so the panel's refusal text and the applied purchase can never diverge.
    TrainerPurchase {
        target: crate::scripts::gui::TrainerTarget,
    },

    /// Buy one replicator item: atomically revalidate the player's live
    /// carried nanite stacks, debit `cost`, then create the item at the
    /// authored output marker. `ReplicatorGui` pre-validates for immediate
    /// refusal text, but the effect handler is authoritative so two
    /// same-frame selections cannot both spend the same balance.
    ReplicatorPurchase {
        cost: i32,
        template_name: String,
        position: Point3<f32>,
        orientation: Quaternion<f32>,
    },

    /// Acquire an O/S upgrade trait at a trait machine: atomically validate
    /// (machine unused, trait not owned, a free slot), record the trait on the
    /// persistent character sheet, mark the machine used (a quest bit keyed by
    /// its stable mission object id), and apply the trait's live effect where
    /// implemented (Tank, Naturally Able). Emitted by `TraitGui`; purchases
    /// are free, per the original.
    AcquireOsTrait {
        trait_id: u8,
        machine: EntityId,
    },

    SetAIProperty {
        entity_id: EntityId,
        update: AIPropertyUpdate,
    },

    /// Debug: force the alertness level of every AI in the mission (broadcast
    /// as a SetAlertness message to each creature's script). `pin` holds the
    /// level against decay and keeps target awareness on the player's live
    /// position until cleared by a non-pinned SetAllAIAlertness.
    SetAllAIAlertness {
        level: AIAlertLevel,
        pin: bool,
    },

    AcquireKeyCard {
        key_card: KeyCard,
    },

    TurnOffTweqs {
        entity_id: EntityId,
    },
    TurnOnTweqs {
        entity_id: EntityId,
    },

    SetUI {
        parent_entity: EntityId,
        handle: GuiHandle,
        world_offset: Vector3<f32>,
        world_size: Vector2<f32>,
        components: Vec<GuiComponentRenderInfo>,
    },

    Multiple(Vec<Effect>),
    // Deprecated:
    // Use Multiple instead
    Combined {
        effects: Vec<Effect>,
    },

    GlobalEffect(GlobalEffect),

    /// Interactive pathfinding test system
    PathfindingTest,

    /// Spawn an entity in front of the player (player position is resolved
    /// by the effect handler; head rotation comes from the input context,
    /// since it isn't stored in the world). `auto_wield` lets flat mode pick
    /// the spawn up as the viewmodel when empty-handed - for item spawns
    /// only, never creatures.
    SpawnInFrontOfPlayer {
        template_id: i32,
        head_rotation: Quaternion<f32>,
        auto_wield: bool,
    },

    /// Debug: spawn the next player weapon in front of the player and wield it
    /// as the flat viewmodel (holstering the previously wielded one). Cycles
    /// the SS2 weapon roster for aim/viewmodel testing.
    DebugCycleWeapon {
        head_rotation: Quaternion<f32>,
    },

    /// Reposition the inventory in front of the player
    PositionInventoryRelativeToPlayer {
        head_rotation: Quaternion<f32>,
    },

    /// Advance every creature to its next animation pose. Debug-only, used by the
    /// `debug_hitbox` scene to inspect per-joint hitbox/ragdoll fit across poses.
    DebugCycleHitboxPose,
}

pub(crate) fn recharge_ammo_to_capacity(gun_state: &mut PropGunState, capacity: i32) {
    gun_state.ammo = gun_state.ammo.max(capacity.max(0));
}

#[cfg(test)]
mod tests {
    use dark::properties::PropGunState;

    use super::recharge_ammo_to_capacity;

    fn gun_state(ammo: i32) -> PropGunState {
        PropGunState {
            ammo,
            condition: 0.75,
            setting: 1,
            modification: 2,
            silence_value: 0.25,
        }
    }

    #[test]
    fn duplicate_same_tick_recharges_are_idempotent_and_preserve_other_state() {
        let mut state = gun_state(0);

        recharge_ammo_to_capacity(&mut state, 100);
        recharge_ammo_to_capacity(&mut state, 100);

        assert_eq!(state.ammo, 100);
        assert_eq!(state.condition, 0.75);
        assert_eq!(state.setting, 1);
        assert_eq!(state.modification, 2);
        assert_eq!(state.silence_value, 0.25);
    }

    #[test]
    fn recharge_does_not_reduce_over_capacity_charge() {
        let mut state = gun_state(125);

        recharge_ammo_to_capacity(&mut state, 100);

        assert_eq!(state.ammo, 125);
    }
}

impl Effect {
    pub fn combine(effects: Vec<Effect>) -> Effect {
        Effect::Combined { effects }
    }
    pub fn flatten(effects: Vec<Effect>) -> Vec<Effect> {
        let mut ret = Vec::new();

        for v in effects {
            match v {
                Effect::NoEffect => (),
                Effect::Multiple(inner_effects) => {
                    ret.append(&mut Self::flatten(inner_effects).clone())
                }
                Effect::Combined {
                    effects: inner_effects,
                } => ret.append(&mut Self::flatten(inner_effects).clone()),
                _ => ret.push(v),
            }
        }

        ret
    }
}
