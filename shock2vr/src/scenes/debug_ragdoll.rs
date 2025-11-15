use cgmath::{InnerSpace, Matrix4, Point3, Quaternion, Vector3, vec3};
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
        DebugSceneBuildOptions, DebugSceneBuilder, DebugSceneFloor, DebugSceneHooks,
        HookedDebugScene,
    },
    scripts::Effect,
    time::Time,
};

const FLOOR_COLOR: Vector3<f32> = Vector3::new(0.15, 0.15, 0.20);
const FLOOR_SIZE: Vector3<f32> = Vector3::new(120.0, 0.5, 120.0);

const IMPULSE_STRENGTH: f32 = 1.0;
const PULL_FORCE: f32 = 1.0;

pub struct DebugRagdollScene;

impl DebugRagdollScene {
    pub fn new(
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Box<dyn GameScene> {
        let builder = DebugSceneBuilder::new("debug_ragdoll")
            .with_floor(DebugSceneFloor::ss2_units(FLOOR_SIZE, FLOOR_COLOR))
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
    slay_timer: f32,
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
            slay_timer: 0.0,
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

        let spawn_position = Point3::new(-5.0, 5.0 / SCALE_FACTOR, -0.0);

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
        self.slay_timer = 1.0;
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
            core.spawn_debug_ragdoll(entity_id);
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
            if let Some(first_body) = core.rag_doll_manager.get_first_ragdoll_body() {
                let impulse = vec3(0.0, IMPULSE_STRENGTH / SCALE_FACTOR, 0.0);
                core.physics.apply_impulse(first_body, impulse);
                println!("Applied upward impulse to ragdoll");
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
                    let pull_force = pull_direction * PULL_FORCE / SCALE_FACTOR;

                    core.physics.apply_force(body_handle, pull_force);
                }
            }
        }

        self.last_left_impulse = input_context.left_hand.trigger_value > 0.5;
        self.last_right_pull = input_context.right_hand.squeeze_value > 0.05;
    }

    fn update_slay_timer(&mut self, delta: f32) {
        if self.pipe_hybrid_spawned && self.slay_timer > 0.0 {
            self.slay_timer -= delta;
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
        self.handle_ragdoll_input(core, input_context);
        self.update_slay_timer(time.elapsed.as_secs_f32());
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
        } else if self.slay_timer <= 0.0 && self.slay_timer > -0.5 {
            self.kill_spawned_entity(
                core,
                global_context,
                game_options,
                asset_cache,
                audio_context,
            );
            self.slay_timer = -1.0;
        }
    }
}
