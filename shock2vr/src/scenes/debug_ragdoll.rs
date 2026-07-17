use cgmath::{InnerSpace, Matrix4, Point3, Quaternion, vec3};
use dark::{SCALE_FACTOR, properties::PropTemplateId};
use engine::{assets::asset_cache::AssetCache, audio::AudioContext};
use shipyard::{EntityId, IntoIter, IntoWithId};

use crate::{
    GameOptions,
    game_scene::GameScene,
    input_context::InputContext,
    mission::{
        GlobalContext, SpawnLocation, entity_creator::CreateEntityOptions,
        mission_core::MissionCore,
    },
    scenes::debug_common::{
        DebugSceneBuildOptions, DebugSceneBuilder, DebugSceneHooks, HookedDebugScene,
    },
    scripts::Effect,
    time::Time,
};

// Ragdoll limb bodies now use a uniform ~1.0 mass (see rag_doll.rs
// TARGET_BODY_MASS), ~1000x heavier than the old point-balls, so these debug
// controls are scaled for that mass.
/// Upward impulse applied to a single ragdoll body (left trigger) - intentionally
/// one body so you can watch the force propagate through the joints to the rest.
const IMPULSE_STRENGTH: f32 = 50.0;
/// Per-body pull force for the debug "gather toward center" control (right squeeze).
const PULL_FORCE: f32 = 120.0;

/// Number of update frames to wait after spawning the pipe hybrid before killing
/// it and spawning the ragdoll. Frame-based (not wall-clock) so the trigger is
/// deterministic under headless stepping, where a single frame can carry a large
/// or irregular delta time.
const KILL_DELAY_FRAMES: u32 = 30;

/// World-space point where the pipe hybrid (and therefore the ragdoll) spawns.
/// Placed directly in front of the default player spawn (which faces -Z) so the
/// corpse is straight ahead, and used as the look-at target for the fixed debug
/// camera so the spawn is always framed.
fn ragdoll_focus_point() -> Point3<f32> {
    Point3::new(-5.0, 5.0 / SCALE_FACTOR, -0.0)
}

pub struct DebugRagdollScene;

impl DebugRagdollScene {
    pub fn new(
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Box<dyn GameScene> {
        let builder = DebugSceneBuilder::new("debug_ragdoll")
            .with_default_floor()
            .with_spawn_location(SpawnLocation::PositionRotation(
                vec3(0.0, 5.0 / SCALE_FACTOR, 0.0),
                Quaternion::new(1.0, 0.0, 0.0, 0.0),
            ));

        let build_options = DebugSceneBuildOptions {
            global_context,
            game_options,
            asset_cache,
            audio_context,
        };

        let core = builder.build_core(build_options);
        let hooks = RagdollHooks::new();
        Box::new(HookedDebugScene::new(core, hooks))
    }
}

struct RagdollHooks {
    pipe_hybrid_spawned: bool,
    frames_since_spawn: u32,
    killed: bool,
    last_left_impulse: bool,
    last_right_pull: bool,
}

impl RagdollHooks {
    fn new() -> Self {
        println!(
            "[debug_ragdoll] Controls:\n\
             - Left trigger to apply upward impulse to ragdoll\n\
             - Right squeeze to pull ragdoll toward center with continuous force"
        );

        Self {
            pipe_hybrid_spawned: false,
            frames_since_spawn: 0,
            killed: false,
            last_left_impulse: false,
            last_right_pull: false,
        }
    }

    fn spawn_pipe_hybrid(
        &mut self,
        core: &mut MissionCore,
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) {
        if self.pipe_hybrid_spawned {
            return;
        }

        let spawn_position = ragdoll_focus_point();

        let spawn_effect = Effect::CreateEntity {
            template_id: -397,
            position: spawn_position,
            orientation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            root_transform: Matrix4::from_translation(vec3(
                spawn_position.x,
                spawn_position.y,
                spawn_position.z,
            )),
            options: CreateEntityOptions::default(),
        };

        let effects = vec![spawn_effect];
        let _global_effects = core.handle_effects(
            effects,
            global_context,
            game_options,
            asset_cache,
            audio_context,
        );

        self.pipe_hybrid_spawned = true;
        self.frames_since_spawn = 0;
        println!("Spawned pipe hybrid for ragdoll testing");
    }

    fn kill_spawned_entity(
        &mut self,
        core: &mut MissionCore,
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) {
        let world = core.world();

        let entities_to_kill = world.run(|v_template_id: shipyard::View<PropTemplateId>| {
            let mut entities_to_kill = Vec::new();

            for (entity_id, template_id) in v_template_id.iter().with_id() {
                if template_id.template_id == -397 {
                    entities_to_kill.push(entity_id);
                    println!("Found pipe hybrid entity {:?} to kill", entity_id);
                }
            }

            entities_to_kill
        });

        for entity_id in entities_to_kill {
            let damage_effect = Effect::AdjustHitPoints {
                entity_id,
                delta: -1000,
            };
            let effects = vec![damage_effect];
            let _global_effects = core.handle_effects(
                effects,
                global_context,
                game_options,
                asset_cache,
                audio_context,
            );
            core.spawn_ragdoll(
                entity_id,
                !game_options
                    .experimental_features
                    .contains("ragdoll_impulse"),
                // Standing-fall spawn, not a crumpled pose: keeps limb
                // self-collision and no lift (the config the settle metrics
                // are calibrated against).
                false,
            );
            println!(
                "Applied massive damage to pipe hybrid entity {:?} for ragdoll testing",
                entity_id
            );
        }
    }

    fn handle_ragdoll_input(&mut self, core: &mut MissionCore, input_context: &InputContext) {
        let ragdoll_bodies = core.rag_doll_manager.get_ragdoll_bodies();

        if ragdoll_bodies.is_empty() {
            return;
        }

        let left_impulse_pressed =
            input_context.left_hand.trigger_value > 0.5 && !self.last_left_impulse;

        if left_impulse_pressed {
            // Single body on purpose: lets you watch the impulse propagate through
            // the joints to the rest of the ragdoll.
            if let Some(first_body) = core.rag_doll_manager.get_first_ragdoll_body() {
                let impulse = vec3(0.0, IMPULSE_STRENGTH, 0.0);
                core.physics.apply_impulse(first_body, impulse);
                println!("Applied upward impulse to a single ragdoll body");
            }
        }

        if input_context.right_hand.squeeze_value > 0.05 {
            let center_position = vec3(0.0, 3.0 / SCALE_FACTOR, 0.0);

            for &body_handle in &ragdoll_bodies {
                if let Some(body_transform) = core.physics.get_body_transform(body_handle) {
                    let body_position = vec3(
                        body_transform.translation.x,
                        body_transform.translation.y,
                        body_transform.translation.z,
                    );

                    let pull_direction = (center_position - body_position).normalize();
                    let pull_force = pull_direction * PULL_FORCE;

                    core.physics.apply_force(body_handle, pull_force);
                }
            }
        }

        self.last_left_impulse = input_context.left_hand.trigger_value > 0.5;
        self.last_right_pull = input_context.right_hand.squeeze_value > 0.05;
    }

    fn advance_kill_counter(&mut self) {
        if self.pipe_hybrid_spawned && !self.killed {
            self.frames_since_spawn += 1;
        }
    }
}

impl DebugSceneHooks for RagdollHooks {
    fn before_update(
        &mut self,
        core: &mut MissionCore,
        time: &Time,
        input_context: &InputContext,
        _asset_cache: &mut AssetCache,
        _game_options: &GameOptions,
    ) {
        let _ = time;
        self.handle_ragdoll_input(core, input_context);
        self.advance_kill_counter();
    }

    fn before_handle_effects(
        &mut self,
        core: &mut MissionCore,
        _effects: &mut Vec<Effect>,
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) {
        if !self.pipe_hybrid_spawned {
            self.spawn_pipe_hybrid(
                core,
                global_context,
                game_options,
                asset_cache,
                audio_context,
            );
        } else if !self.killed && self.frames_since_spawn >= KILL_DELAY_FRAMES {
            self.kill_spawned_entity(
                core,
                global_context,
                game_options,
                asset_cache,
                audio_context,
            );
            self.killed = true;
        }
    }
}
