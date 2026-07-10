use cgmath::{Matrix4, Point3, Quaternion, Vector2, Vector3, Vector4};
use dark::{
    EnvSoundQuery,
    motion::{MotionQueryItem, MotionQuerySelectionStrategy},
    properties::{AIAlertLevel, AIMode, KeyCard, QuestBitValue},
};
use engine::audio::AudioHandle;
use shipyard::EntityId;

use crate::{
    gui::{GuiComponentRenderInfo, GuiHandle},
    mission::entity_creator::CreateEntityOptions,
    vr_config::Handedness,
};

use super::Message;

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
    },

    // Test the reload functionality (as if saving + loading)
    TestReload,

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

    /// Adjust a weapon's current clip ammo (`PropGunState.ammo`) by `delta`
    /// (negative to consume). No-op for entities without a gun state.
    AdjustAmmo {
        entity_id: EntityId,
        delta: i32,
    },

    /// Cycle the player's wielded weapon to its next ammo type (the next
    /// `Projectile` link). No-op when no weapon is wielded or it has fewer than
    /// two projectile links.
    CycleAmmo,

    /// Toggle the flat-mode "use" (metagame) mode - cursor-driven UI over the
    /// 3D view, as the original game does on Tab. Currently only tracks the
    /// mode (introspectable via the debug runtime's `GET /v1/ui`); the cursor
    /// and panel presentation land with the flat UI host. No-op in VR.
    /// See `projects/flat-ui.md`.
    ToggleUseMode,

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

    /// Reload the player's wielded weapon: refill its clip (`PropGunState.ammo`)
    /// to the magazine capacity (`PropBaseGunDesc.clip`). No-op when no weapon is
    /// wielded or it has no gun state / clip. (Reserve ammo is unlimited for now -
    /// there is no inventory ammo model yet.)
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
        template_name: String,
        position: Point3<f32>,
        orientation: Quaternion<f32>,
    },

    CreateEntity {
        template_id: i32,
        position: Point3<f32>,
        orientation: Quaternion<f32>,
        root_transform: Matrix4<f32>,
        options: CreateEntityOptions,
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

    SetAIProperty {
        entity_id: EntityId,
        update: AIPropertyUpdate,
    },

    /// Debug: force the alertness level of every AI in the mission (broadcast
    /// as a SetAlertness message to each creature's script)
    SetAllAIAlertness {
        level: AIAlertLevel,
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
    /// as the flat viewmodel (dropping the previously wielded one). Cycles the
    /// SS2 weapon roster for aim/viewmodel testing.
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
