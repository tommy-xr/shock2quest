use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::BufReader,
    rc::Rc,
    time::{Duration, SystemTime},
};

use cgmath::{
    num_traits::ToPrimitive, vec3, InnerSpace, Matrix4, Point3, Quaternion, Rotation, Rotation3,
    SquareMatrix, Transform, Vector2, Vector3,
};
use cgmath::{EuclideanSpace, Zero};

use crate::mission::CullingInfo;
use crate::mission::VisibilityEngine;
use crate::SpawnLocation;
use crate::{mission::entity_creator, scripts::AIPropertyUpdate};

use dark::{
    audio::SongPlayer,
    gamesys::Gamesys,
    importers::{ANIMATION_CLIP_IMPORTER, AUDIO_IMPORTER, MODELS_IMPORTER, SONG_IMPORTER},
    mission::{room_database::RoomDatabase, SongParams},
    model::Model,
    motion::{AnimationEvent, AnimationPlayer, MotionDB, MotionQuery, MotionQueryItem},
    properties::{
        AmbientSoundFlags, Link, LinkDefinition, LinkDefinitionWithData, Links, PhysicsModelType,
        PropAIAlertness, PropAIMode, PropAmbientHacked, PropCreature, PropFrameAnimState,
        PropHasRefs, PropLocalPlayer, PropModelName, PropMotionActorTags, PropParticleGroup,
        PropParticleLaunchInfo, PropPhysDimensions, PropPhysInitialVelocity, PropPhysState,
        PropPhysType, PropPosition, PropRenderType, PropScripts, PropTeleported, PropTripFlags,
        PropertyDefinition, RenderType, ToLink, TripFlags, WrappedEntityId,
    },
    ss2_entity_info::{self, SystemShock2EntityInfo},
    tag_database::{TagQuery, TagQueryItem},
    BitmapAnimation, SCALE_FACTOR,
};
use engine::{
    assets::asset_cache::AssetCache,
    audio::{AudioChannel, AudioContext, AudioHandle},
    game_log, profile,
    scene::{
        light::SpotLight, quad, BillboardMaterial, ParticleSystem, SceneObject, VertexPosition,
    },
    texture::TextureTrait,
};
use physics::PhysicsWorld;
use rand::{
    distributions::WeightedIndex, prelude::Distribution, seq::SliceRandom, thread_rng, Rng,
};
use rapier3d::prelude::{Collider, RigidBodyHandle};
use scripts::ScriptWorld;

use shipyard::*;
use shipyard::{self, View, World};
use tracing::{info, trace, warn};

use crate::{
    creature::{get_creature_definition, HitBoxManager},
    game_scene::AmbientAudioState,
    gui::GuiManager,
    hud::{draw_item_name, draw_item_outline},
    input_context::{self, InputContext},
    inventory::PlayerInventoryEntity,
    mission::{entity_populator::EntityPopulator, SpatialQueryEngine},
    physics::{self, PlayerHandle},
    quest_info::QuestInfo,
    runtime_props::{
        RuntimePropDoNotSerialize, RuntimePropJointTransforms, RuntimePropTransform,
        RuntimePropVhots,
    },
    save_load::HeldItemSaveData,
    scripts::{
        self,
        internal_fast_projectile::InternalFastProjectileScript,
        script_util::{get_all_links_with_template, get_environmental_sound_query},
        speech_registry::SpeechVoiceRegistry,
        Effect, GlobalEffect, Message, MessagePayload,
    },
    systems::{run_bitmap_animation, run_tweq, turn_off_tweqs, turn_on_tweqs},
    teleport::TeleportSystem,
    time::Time,
    util::{get_email_sound_file, has_refs, vec3_to_point3},
    virtual_hand::{VirtualHand, VirtualHandEffect},
    vr_config, GameOptions,
};

use crate::mission::entity_creator::{CreateEntityOptions, EntityCreationInfo};
pub use crate::resource_path;

#[derive(Unique, Clone)]
pub struct PlayerInfo {
    pub pos: Vector3<f32>,
    pub rotation: Quaternion<f32>,
    pub entity_id: EntityId,

    pub left_hand_entity_id: Option<EntityId>,
    pub right_hand_entity_id: Option<EntityId>,
    pub inventory_entity_id: EntityId,
}

#[derive(Unique, Clone)]
pub struct EffectQueue {
    effects: Vec<Effect>,
}

pub struct DebugLine {
    pub start: Point3<f32>,
    pub end: Point3<f32>,
    pub color: Vector3<f32>,
    pub remaining_life_in_seconds: f32,
}

#[derive(Clone)]
pub struct EntityMetadata {
    pub template_id: i32,
    pub obj_icon: Option<String>,
    pub obj_short_name: Option<String>,
    #[allow(dead_code)]
    pub obj_name: Option<String>,
}

#[derive(Unique, Clone)]
pub struct GlobalEntityMetadata(pub HashMap<String, EntityMetadata>);

#[derive(Unique, Clone)]
pub struct GlobalTemplateIdMap(pub HashMap<i32, WrappedEntityId>);

impl EffectQueue {
    pub fn push(&mut self, effect: Effect) {
        self.effects.push(effect);
    }

    pub fn flush(&mut self) -> Vec<Effect> {
        let prev = self.effects.clone();
        self.effects = vec![];
        prev
    }
}

pub struct MissionCore {
    pub level_name: String,
    pub gui: GuiManager,
    pub hit_boxes: HitBoxManager,
    pub debug_lines: Vec<DebugLine>,
    pub entity_info: SystemShock2EntityInfo,
    pub physics: PhysicsWorld,
    pub script_world: ScriptWorld,
    pub scene_objects: Vec<SceneObject>,
    pub id_to_animation_player: HashMap<EntityId, AnimationPlayer>,
    pub id_to_model: HashMap<EntityId, Model>,
    pub id_to_bitmap: HashMap<EntityId, Rc<BitmapAnimation>>,
    pub id_to_physics: HashMap<EntityId, RigidBodyHandle>,
    pub id_to_particle_system: HashMap<EntityId, ParticleSystem>,
    #[allow(dead_code)]
    pub template_to_entity_id: HashMap<i32, WrappedEntityId>,
    pub template_name_to_template_id: HashMap<String, EntityMetadata>,
    pub obj_map: HashMap<i32, String>,
    pub world: World,
    pub player_handle: PlayerHandle,
    pub spatial_data: Option<Box<dyn SpatialQueryEngine>>,
    pub left_hand: VirtualHand,
    pub right_hand: VirtualHand,
    pub visibility_engine: Box<dyn VisibilityEngine>,
    pub teleport_system: TeleportSystem,
    pub pending_entity_triggers: Vec<String>,
}

pub struct GlobalContext {
    pub properties: Vec<Box<dyn PropertyDefinition<BufReader<File>>>>,
    pub links: Vec<Box<dyn LinkDefinition>>,
    pub links_with_data: Vec<Box<dyn LinkDefinitionWithData>>,
    pub gamesys: Gamesys,
    pub motiondb: MotionDB,
}

pub struct AbstractMission {
    pub scene_objects: Vec<SceneObject>,
    pub song_params: SongParams,
    pub room_db: RoomDatabase,
    pub physics_geometry: Option<Collider>,
    pub spatial_data: Option<Box<dyn SpatialQueryEngine>>,
    pub entity_info: SystemShock2EntityInfo,
    pub obj_map: HashMap<i32, String>,
    pub visibility_engine: Box<dyn VisibilityEngine>,
}

impl MissionCore {
    pub fn load(
        mission: String,
        abstract_mission: AbstractMission,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
        global_context: &GlobalContext,
        spawn_loc: SpawnLocation,
        quest_info: QuestInfo,
        entity_populator: Box<dyn EntityPopulator>,
        held_item_save_data: HeldItemSaveData,
        game_options: &GameOptions,
    ) -> MissionCore {
        let game_entity_info = &global_context.gamesys;
        let _motiondb = &global_context.motiondb;

        let mut world = World::new();
        let start = SystemTime::now();
        info!("starting level load");
        let scene = abstract_mission.scene_objects;
        let duration: Duration = start.elapsed().unwrap();
        info!("loading level took {}s", duration.as_secs_f32());

        let entity_info =
            ss2_entity_info::merge_with_gamesys(&abstract_mission.entity_info, game_entity_info);

        let speech_registry = SpeechVoiceRegistry::from_entity_info(&entity_info);

        let mut id_to_model = HashMap::new();
        let mut id_to_animation_player = HashMap::new();

        // Create player
        let player_entity = world.add_entity((PropLocalPlayer {}, RuntimePropDoNotSerialize {}));

        // Create a map of template name (ie 'HE Explosion' to the template id).
        // This is important for creating entities based on template name
        let template_name_to_template_id = create_template_name_map(game_entity_info);

        world.add_unique(GlobalEntityMetadata(template_name_to_template_id.clone()));
        world.add_unique(Time::default());
        world.add_unique(speech_registry);

        // ** Entity creation

        let template_to_entity_id = entity_populator.populate(
            &entity_info,
            &abstract_mission.entity_info,
            &abstract_mission.obj_map,
            &mut world,
        );

        // Instantiate held items
        let mut left_hand = VirtualHand::new(vr_config::Handedness::Left);
        let mut right_hand = VirtualHand::new(vr_config::Handedness::Right);
        let (left_hand_entity, right_hand_entity, maybe_inventory_entity) =
            held_item_save_data.instantiate(&mut world);

        // Instantiate inventory
        // TODO: This should be move into the held_item_save_data
        let inventory = if let Some(inv_entity) = maybe_inventory_entity {
            inv_entity
        } else {
            PlayerInventoryEntity::create(&mut world)
        };

        // HACK: Re-add runtime prop transform for inventory
        world.add_component(
            inventory,
            RuntimePropTransform(Matrix4::from_translation(vec3(0.0, 1.0, 0.0))),
        );
        world.add_component(inventory, PlayerInventoryEntity {});

        world.add_unique(GlobalTemplateIdMap(template_to_entity_id.clone()));

        // Start background music
        initialize_background_music(&abstract_mission.song_params, asset_cache, audio_context);

        let mut entities_to_instantiate = HashSet::new();

        // Create rooms
        create_room_entities(
            &abstract_mission.room_db,
            &template_to_entity_id,
            &mut world,
            &mut entities_to_instantiate,
        );

        // Get the set of entities with PropPosition to be materialized
        world.run(
            |v_pos: View<dark::properties::PropPosition>,
             v_template_id: View<dark::properties::PropTemplateId>| {
                for (id, (_pos, template_id)) in (&v_pos, &v_template_id).iter().with_id() {
                    entities_to_instantiate.insert((id, template_id.template_id));
                }
            },
        );

        let mut physics = PhysicsWorld::new();
        let mut id_to_physics = HashMap::new();
        let mut id_to_bitmap = HashMap::new();
        let mut script_world = ScriptWorld::new();

        let world_entity_id = world.add_entity(RuntimePropDoNotSerialize {});
        if let Some(collider) = abstract_mission.physics_geometry {
            physics.add_collider(world_entity_id, collider);
        }

        // Finally, instantiate these entities
        for (entity_id, template_id) in entities_to_instantiate {
            let created_entity = entity_creator::initialize_entity(
                entity_id,
                template_id,
                &mut world,
                &mut physics,
                asset_cache,
                &mut script_world,
                &entity_info,
                &abstract_mission.obj_map,
                &template_to_entity_id,
                CreateEntityOptions::default(),
            );

            Self::finish_instantiating_entity(
                &mut id_to_model,
                &mut id_to_bitmap,
                &mut id_to_physics,
                &mut id_to_animation_player,
                &mut physics,
                &mut world,
                &mut script_world,
                created_entity,
                Matrix4::identity(),
            );
        }

        // If the player is holding anything, we should un-physical it

        if let Some(entity_id) = left_hand_entity {
            // panic!("got an lent: {:?}", entity_id);
            left_hand = left_hand.grab_entity(&world, entity_id);
            make_un_physical2(&mut id_to_physics, &mut physics, entity_id);
        };

        if let Some(entity_id) = right_hand_entity {
            // panic!("got an rent: {:?}", entity_id);
            right_hand = right_hand.grab_entity(&world, entity_id);
            make_un_physical2(&mut id_to_physics, &mut physics, entity_id);
        };

        let (start_pos, start_rotation) = spawn_loc.calculate_start_position(
            &world,
            &abstract_mission.entity_info,
            &template_to_entity_id,
        );

        let player_handle = physics.create_player(start_pos, player_entity);

        world.add_unique(PlayerInfo {
            rotation: start_rotation,
            pos: start_pos,
            entity_id: player_entity,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory,
        });

        world.add_unique(quest_info);

        world.add_unique(EffectQueue {
            effects: Vec::new(),
        });

        // Initialize teleport system based on game options
        let teleport_system = if game_options.experimental_features.contains("teleport") {
            let teleport_config = crate::teleport::TeleportConfig {
                enabled: true,
                button_mapping: crate::teleport::TeleportButton::Trigger,
                trigger_threshold: 0.5,
                max_distance: 20.0,
                ..Default::default()
            };
            TeleportSystem::new(teleport_config)
        } else {
            let teleport_config = crate::teleport::TeleportConfig {
                enabled: false,
                ..Default::default()
            };
            TeleportSystem::new(teleport_config)
        };

        MissionCore {
            left_hand,
            right_hand,
            level_name: mission,
            entity_info,
            script_world,
            id_to_model,
            id_to_animation_player,
            id_to_bitmap,
            id_to_particle_system: HashMap::new(),
            template_name_to_template_id,
            scene_objects: scene,
            physics,
            world,
            id_to_physics,
            template_to_entity_id,
            player_handle,
            spatial_data: abstract_mission.spatial_data,
            debug_lines: Vec::new(),
            gui: GuiManager::new(),
            hit_boxes: HitBoxManager::new(),
            visibility_engine: abstract_mission.visibility_engine,
            teleport_system,
            pending_entity_triggers: Vec::new(),
            obj_map: abstract_mission.obj_map,
        }
    }

    pub fn update(
        &mut self,
        time: &Time,
        asset_cache: &mut AssetCache,
        input_context: &input_context::InputContext,
        game_options: &GameOptions,
        command_effects: Vec<Effect>,
    ) -> Vec<Effect> {
        let _ = self.world.remove_unique::<Time>();
        self.world.add_unique(time.clone());
        let mut effects = command_effects;

        // Update teleport system and add effects (only if experimental flag enabled)
        if game_options.experimental_features.contains("teleport") {
            let teleport_effects = self.teleport_system.update(input_context);
            effects.extend(teleport_effects);
        }

        // Player movement logic
        let delta_time = time.elapsed.as_secs_f32();
        let player = {
            let player_info = self.world.borrow::<UniqueView<PlayerInfo>>().unwrap();
            player_info.clone()
        };

        let rot_speed = 2.0;
        let additional_rotation = cgmath::Quaternion::from_axis_angle(
            cgmath::vec3(0.0, 1.0, 0.0),
            cgmath::Rad(input_context.left_hand.thumbstick.x * delta_time * rot_speed),
        );

        let new_rotation = player.rotation * additional_rotation;

        let dir = new_rotation * input_context.head.rotation;
        let move_thumbstick_value = input_context.right_hand.thumbstick;
        let forward = dir.rotate_vector(cgmath::vec3(
            -delta_time * move_thumbstick_value.x * 25. / dark::SCALE_FACTOR,
            0.0,
            -delta_time * move_thumbstick_value.y * 25. / dark::SCALE_FACTOR,
        ));

        let up_value = input_context.left_hand.thumbstick.y / dark::SCALE_FACTOR;

        let (new_character_pos, collision_events) = {
            profile!(
                "shock2.update.physics",
                self.physics.update(
                    forward + cgmath::vec3(0.0, up_value, 0.0),
                    &mut self.player_handle,
                )
            )
        };

        // Clear forces
        self.physics.clear_forces();

        let (left_hand_entity_id, right_hand_entity_id) = {
            (
                self.left_hand.get_held_entity(),
                self.right_hand.get_held_entity(),
            )
        };

        // Update player info
        let mut player_info = self.world.borrow::<UniqueViewMut<PlayerInfo>>().unwrap();
        player_info.pos = new_character_pos;
        player_info.rotation = new_rotation;
        player_info.left_hand_entity_id = left_hand_entity_id;
        player_info.right_hand_entity_id = right_hand_entity_id;
        drop(player_info);

        // Handle collision events
        for ce in collision_events {
            info!("event: {:?}", ce);

            match ce {
                physics::CollisionEvent::BeginIntersect {
                    sensor_id,
                    entity_id,
                } => {
                    self.script_world.dispatch(Message {
                        to: sensor_id,
                        payload: MessagePayload::SensorBeginIntersect { with: entity_id },
                    });
                }
                physics::CollisionEvent::EndIntersect {
                    sensor_id,
                    entity_id,
                } => {
                    self.script_world.dispatch(Message {
                        to: sensor_id,
                        payload: MessagePayload::SensorEndIntersect { with: entity_id },
                    });
                }
                physics::CollisionEvent::CollisionStarted {
                    entity1_id,
                    entity2_id,
                } => {
                    self.script_world.dispatch(Message {
                        to: entity1_id,
                        payload: MessagePayload::Collided { with: entity2_id },
                    });
                    self.script_world.dispatch(Message {
                        to: entity2_id,
                        payload: MessagePayload::Collided { with: entity1_id },
                    });
                }
            }
        }

        // Update PropTeleported entities
        self.world.run(
            |mut v_teleported: ViewMut<dark::properties::PropTeleported>| {
                let mut ents_to_remove = Vec::new();
                for (id, door) in (&mut v_teleported).iter().with_id() {
                    door.countdown_timer -= time.elapsed.as_secs_f32();

                    if door.countdown_timer < 0.0 {
                        ents_to_remove.push(id);
                    }
                }

                for id in ents_to_remove {
                    v_teleported.remove(id);
                }
            },
        );

        let (player_pos, player_rot) = {
            let player_info = self.world.borrow::<UniqueView<PlayerInfo>>().unwrap();
            (player_info.pos, player_info.rotation)
        };

        self.debug_lines.iter_mut().for_each(|p| {
            p.remaining_life_in_seconds -= time.elapsed.as_secs_f32();
        });

        self.debug_lines
            .retain(|p| p.remaining_life_in_seconds > 0.0);

        self.update_animations(time);

        self.hit_boxes.update(
            &mut self.world,
            &mut self.physics,
            &mut self.script_world,
            &self.id_to_model,
            &mut self.id_to_physics,
        );

        self.update_avatar_hands(asset_cache, player_pos, player_rot, input_context);

        // Sync up the position of all the physics objects
        // The timing of this is important - things like the GUI rendering depend on an up-to-date position
        // from physics
        self.synchronize_physics_positions();

        // Update scripts
        let mut script_effects = profile!(
            scope: "game", level: DEBUG, "script_world.update",
            self.script_world.update(&self.world, &self.physics, time)
        );
        effects.append(&mut script_effects);

        // Handle any pending entity triggers now that scripts are initialized
        if !self.pending_entity_triggers.is_empty() {
            println!(
                "Processing {} pending entity triggers after script initialization",
                self.pending_entity_triggers.len()
            );
            let pending_triggers = self.pending_entity_triggers.drain(..).collect::<Vec<_>>();
            for entity_name in pending_triggers {
                println!("Triggering delayed entity: {}", entity_name);
                let messages = self.trigger_entity_by_name_internal(entity_name);
                for message in messages {
                    println!("Dispatching message: {:?}", message);
                    self.script_world.dispatch(message);
                }
            }
        }

        self.world.run(run_tweq);
        self.world.run(run_bitmap_animation);

        self.gui.update();

        let mut current_effects = self.world.borrow::<UniqueViewMut<EffectQueue>>().unwrap();
        effects.append(&mut current_effects.flush());

        // Update particle systems
        self.world.run(
            |prop_particle_group: View<PropParticleGroup>,
             prop_particle_launch_info: View<PropParticleLaunchInfo>,
             transform: View<RuntimePropTransform>| {
                for (id, (pg, launch_info, transform)) in
                    (&prop_particle_group, &prop_particle_launch_info, &transform)
                        .iter()
                        .with_id()
                {
                    let particle_system =
                        self.id_to_particle_system.entry(id).or_insert_with(|| {
                            ParticleSystem::new()
                                .with_lifetime(launch_info.min_time, launch_info.max_time)
                                .with_velocity(
                                    launch_info.vel_min / SCALE_FACTOR,
                                    launch_info.vel_max / SCALE_FACTOR,
                                )
                                .with_acceleration(pg.gravity / SCALE_FACTOR)
                                .with_launch_bounding_box(
                                    launch_info.loc_min / SCALE_FACTOR,
                                    launch_info.loc_max / SCALE_FACTOR,
                                )
                                .with_particle_size(
                                    2.0 * pg.size / SCALE_FACTOR,
                                    2.0 * pg.size / SCALE_FACTOR,
                                )
                                .with_num_particles(pg.num as usize)
                                .with_launch_time(Duration::from_secs_f32(pg.launch_time))
                                .with_alpha(pg.a as f32 / 255.0)
                                .with_fade_time(pg.fade_time)
                        });
                    particle_system.update(time.elapsed, transform.0);
                }
            },
        );

        effects
    }

    ///
    /// synchronize_physics_positions
    ///
    /// Populate the PropPosition and RuntimePropTransform components,
    /// based on the current values in the physics engine
    fn synchronize_physics_positions(&mut self) {
        {
            let v_scale = self
                .world
                .borrow::<View<dark::properties::PropScale>>()
                .unwrap();
            let mut v_transform = self
                .world
                .borrow::<ViewMut<RuntimePropTransform>>()
                .unwrap();
            let mut v_prop_position = self.world.borrow::<ViewMut<PropPosition>>().unwrap();
            let v_entities = self.world.borrow::<EntitiesView>().unwrap();
            for (entity_id, handle) in &self.id_to_physics {
                let scale = v_scale
                    .get(*entity_id)
                    .map(|p| p.0)
                    .unwrap_or(vec3(1.0, 1.0, 1.0));
                let position = self.physics.get_position(*handle).unwrap();
                let rotation = self.physics.get_rotation(*handle).unwrap();
                let scale_xform =
                    Matrix4::from_nonuniform_scale(scale.x.abs(), scale.y.abs(), scale.z.abs());
                let translation_xform = Matrix4::from_translation(position);
                let rotation_xform = Matrix4::from(rotation);
                let xform = translation_xform * rotation_xform * scale_xform;
                v_entities.add_component(
                    *entity_id,
                    &mut v_prop_position,
                    PropPosition {
                        position,
                        rotation,
                        cell: 0,
                    },
                );
                v_entities.add_component(*entity_id, &mut v_transform, RuntimePropTransform(xform));
            }
        };
    }

    fn update_animations(&mut self, time: &Time) {
        for (id, player) in self.id_to_animation_player.iter_mut() {
            // self.id_to_animation_player.entry(*id).and_modify(|player| {
            //     *player = AnimationPlayer::update(player, time.elapsed);
            // });
            let (new_player, flags, events, velocity) =
                AnimationPlayer::update(player, time.elapsed);
            *player = new_player;

            if let Some(model) = self.id_to_model.get(id) {
                let joint_transforms = model.get_joint_transforms(player);
                self.world
                    .add_component(*id, RuntimePropJointTransforms(joint_transforms));
            }

            let v_transform = self.world.borrow::<View<RuntimePropTransform>>().unwrap();
            let maybe_transform = v_transform.get(*id);
            let curr_velocity = self
                .physics
                .get_velocity(*id)
                .unwrap_or(vec3(0.0, 0.0, 0.0));
            if let Ok(transform) = maybe_transform {
                let adj_velocity =
                    transform
                        .0
                        .transform_vector(vec3(velocity.z, curr_velocity.y, -velocity.x));
                self.physics.set_velocity(*id, adj_velocity * 1.0);
            }

            if !flags.is_empty() {
                self.script_world.dispatch(Message {
                    to: *id,
                    payload: MessagePayload::AnimationFlagTriggered {
                        motion_flags: flags,
                    },
                })
            }

            for event in events {
                match event {
                    AnimationEvent::Completed => self.script_world.dispatch(Message {
                        to: *id,
                        payload: MessagePayload::AnimationCompleted,
                    }),
                    AnimationEvent::DirectionChanged(ang) => {
                        game_log!(DEBUG, "Animation direction changed: {:?}", ang);
                        let maybe_current_rotation = self.physics.get_rotation2(*id);
                        if let Some(current_rotation) = maybe_current_rotation {
                            let new_rotation =
                                current_rotation * Quaternion::from_angle_y(ang * -0.5);
                            self.physics.set_rotation2(*id, new_rotation);
                        }
                    }
                    AnimationEvent::VelocityChanged(_velocity) => (),
                }
            }
        }
    }

    pub fn slay_entity(&mut self, entity_id: EntityId, asset_cache: &mut AssetCache) -> bool {
        let world = &self.world;
        let flinderize_links = get_all_links_with_template(world, entity_id, |link| match link {
            Link::Flinderize(opts) => Some(*opts),
            _ => None,
        });

        let corpse_links = get_all_links_with_template(world, entity_id, |link| match link {
            Link::Corpse(opts) => Some(*opts),
            _ => None,
        });

        let did_slay = true;

        if let Some(handle) = &self.id_to_physics.get(&entity_id) {
            let position = self.physics.get_position(**handle).unwrap();
            let rotation = self.physics.get_rotation(**handle).unwrap();

            for (template_id, _flinderize_options) in flinderize_links {
                // let flinderize_position = flinderize.position;
                // let flinderize_orientation = flinderize.orientation;

                self.create_entity_with_position(
                    asset_cache,
                    template_id,
                    vec3_to_point3(position),
                    rotation,
                    Matrix4::identity(),
                    CreateEntityOptions::default(),
                );
                //did_slay = true;
            }

            for (template_id, _corpse_options) in corpse_links {
                // let flinderize_position = flinderize.position;
                // let flinderize_orientation = flinderize.orientation;

                self.create_entity_with_position(
                    asset_cache,
                    template_id,
                    vec3_to_point3(position),
                    rotation,
                    Matrix4::identity(),
                    CreateEntityOptions::default(),
                );
                //did_slay = true;
            }
        }

        did_slay
    }

    pub fn create_entity_by_template_name(
        &mut self,
        asset_cache: &mut AssetCache,
        template_name: &str,
        position: Point3<f32>,
        orientation: Quaternion<f32>,
    ) -> Option<EntityCreationInfo> {
        let template_name_lowercase = template_name.to_ascii_lowercase();
        let maybe_template_id = self
            .template_name_to_template_id
            .get(&template_name_lowercase)
            .cloned();

        if let Some(template_id) = maybe_template_id {
            Some(self.create_entity_with_position(
                asset_cache,
                template_id.template_id,
                position,
                orientation,
                Matrix4::identity(),
                CreateEntityOptions::default(),
            ))
        } else {
            None
        }
    }

    pub fn make_un_physical(&mut self, entity_id: EntityId) {
        let current_entity = self.id_to_physics.get(&entity_id);
        if current_entity.is_none() {
            return;
        }

        self.physics.remove(entity_id);
        self.id_to_physics.remove(&entity_id);
    }

    pub fn make_physical(&mut self, entity_id: EntityId) {
        let current_entity = self.id_to_physics.get(&entity_id);
        if current_entity.is_some() {
            return;
        }

        let maybe_model = self.id_to_model.get(&entity_id);

        let maybe_phys_obj = entity_creator::create_physics_representation(
            &mut self.world,
            &mut self.physics,
            &maybe_model,
            entity_id,
        );

        if let Some(phys_obj) = maybe_phys_obj {
            self.id_to_physics.insert(entity_id, phys_obj);
        }
    }

    pub fn set_entity_position_rotation(
        &mut self,
        entity_id: EntityId,
        position: Vector3<f32>,
        rotation: Quaternion<f32>,
        scale: Vector3<f32>,
    ) {
        if let Some(rigid_body_handle) = self.id_to_physics.get(&entity_id) {
            self.physics
                .set_position_rotation(*rigid_body_handle, position, rotation);
        } else {
            let translation_matrix = Matrix4::from_translation(position);
            let rotation_matrix = Matrix4::<f32>::from(rotation);
            let scale_matrix = Matrix4::from_nonuniform_scale(scale.x, scale.y, scale.z);
            let xform = translation_matrix * rotation_matrix * scale_matrix;

            let v_entities = self.world.borrow::<EntitiesView>().unwrap();
            let mut v_transform = self
                .world
                .borrow::<ViewMut<RuntimePropTransform>>()
                .unwrap();

            let mut v_prop_position = self.world.borrow::<ViewMut<PropPosition>>().unwrap();

            v_entities.add_component(entity_id, &mut v_transform, RuntimePropTransform(xform));
            v_entities.add_component(
                entity_id,
                &mut v_prop_position,
                PropPosition {
                    position,
                    rotation,
                    cell: 0,
                },
            );
        }
    }

    pub fn create_entity_with_position(
        &mut self,
        asset_cache: &mut AssetCache,
        template_id: i32,
        position: Point3<f32>,
        orientation: Quaternion<f32>,
        root_transform: Matrix4<f32>,
        additional_options: CreateEntityOptions,
    ) -> EntityCreationInfo {
        let created_entity = {
            entity_creator::create_entity_with_position(
                template_id,
                position,
                orientation,
                root_transform,
                &mut self.world,
                &mut self.physics,
                asset_cache,
                &mut self.script_world,
                &self.entity_info,
                &self.obj_map,
                &self.template_to_entity_id,
                additional_options,
            )
        };

        Self::finish_instantiating_entity(
            &mut self.id_to_model,
            &mut self.id_to_bitmap,
            &mut self.id_to_physics,
            &mut self.id_to_animation_player,
            &mut self.physics,
            &mut self.world,
            &mut self.script_world,
            created_entity,
            root_transform,
        )
    }

    fn finish_instantiating_entity(
        id_to_model: &mut HashMap<EntityId, Model>,
        id_to_bitmap: &mut HashMap<EntityId, Rc<BitmapAnimation>>,
        id_to_physics: &mut HashMap<EntityId, RigidBodyHandle>,
        id_to_animation_player: &mut HashMap<EntityId, AnimationPlayer>,
        physics: &mut PhysicsWorld,
        world: &mut World,
        script_world: &mut ScriptWorld,
        created_entity: EntityCreationInfo,
        root_transform: Matrix4<f32>,
    ) -> EntityCreationInfo {
        let ret = created_entity.clone();

        if let Some((model, maybe_animation_player)) = created_entity.model {
            id_to_model.insert(created_entity.entity_id, model);

            if let Some(animation_player) = maybe_animation_player {
                id_to_animation_player.insert(created_entity.entity_id, animation_player);
            }
        }

        if let Some(bitmap_animation) = created_entity.bitmap_animation {
            id_to_bitmap.insert(created_entity.entity_id, bitmap_animation);
        }

        let v_initial_velocity = world.borrow::<View<PropPhysInitialVelocity>>().unwrap();
        if let Some(rigid_body) = created_entity.rigid_body {
            let initial_velocity = v_initial_velocity
                .get(created_entity.entity_id)
                // Not sure why the coordinate system is different for projectile launch?
                .map(|v| vec3(v.0.z, v.0.y, v.0.x))
                //.map(|v| vec3(v.0.z.abs(), v.0.y.abs(), v.0.x.abs()))
                .unwrap_or(vec3(0.0, 0.0, 0.0));

            let mag = initial_velocity.magnitude();
            let x_velocity = root_transform.transform_vector(vec3(0.0, 0.0, mag));
            if initial_velocity.magnitude() > 80.0 {
                // Use raycast strategy for fast moving objects
                script_world.add_entity2(
                    created_entity.entity_id,
                    Box::new(InternalFastProjectileScript::new(x_velocity)),
                );
                // HACK: Don't use physics for these entities...
                physics.remove(created_entity.entity_id);
            } else {
                id_to_physics.insert(created_entity.entity_id, rigid_body);
                physics.set_velocity(created_entity.entity_id, (x_velocity / SCALE_FACTOR) * 1.5);
            }
        };

        ret
    }

    pub fn remove_entity(&mut self, entity_id: EntityId) {
        // TODO: gui - remove entity
        self.hit_boxes.remove_entity(
            entity_id,
            &mut self.world,
            &mut self.script_world,
            &mut self.physics,
            &mut self.id_to_physics,
        );

        self.script_world.remove_entity(entity_id);
        self.id_to_bitmap.remove(&entity_id);
        self.id_to_model.remove(&entity_id);
        self.id_to_physics.remove(&entity_id);
        self.physics.remove(entity_id);

        self.world.delete_entity(entity_id);
    }

    pub fn handle_effects(
        &mut self,
        effects: Vec<Effect>,
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Vec<GlobalEffect> {
        let mut global_effects = Vec::new();
        let player_entity = {
            let player_info = self.world.borrow::<UniqueView<PlayerInfo>>().unwrap();
            player_info.entity_id
        };

        for effect in effects {
            match effect {
                Effect::AcquireKeyCard { key_card } => {
                    let mut quests = self.world.borrow::<UniqueViewMut<QuestInfo>>().unwrap();
                    quests.add_key_card(key_card);
                    drop(quests);
                }

                Effect::AdjustHitPoints { entity_id, delta } => {
                    let mut v_hit_points = self
                        .world
                        .borrow::<ViewMut<dark::properties::PropHitPoints>>()
                        .unwrap();

                    if let Ok(hit_points) = (&mut v_hit_points).get(entity_id) {
                        hit_points.hit_points += delta;
                    }
                }

                Effect::AwardXP { amount } => {
                    warn!("!! TODO !!: Award XP {}", amount);
                }

                Effect::DrawDebugLines { lines } => {
                    if game_options.debug_draw {
                        for line in lines {
                            self.debug_lines.push(DebugLine {
                                start: line.0,
                                end: line.1,
                                color: vec3(line.2.x, line.2.y, line.2.z),
                                remaining_life_in_seconds: 0.1,
                            })
                        }
                    }
                }

                Effect::CreateEntityByTemplateName {
                    template_name,
                    position,
                    orientation,
                } => {
                    self.create_entity_by_template_name(
                        asset_cache,
                        &template_name,
                        position,
                        orientation,
                    );
                }

                Effect::CreateEntity {
                    template_id,
                    position,
                    orientation,
                    root_transform,
                    options,
                } => {
                    self.create_entity_with_position(
                        asset_cache,
                        template_id,
                        position,
                        orientation,
                        root_transform,
                        options,
                    );
                }
                Effect::DropEntityInfo {
                    parent_entity_id,
                    dropped_entity_id,
                } => {
                    let mut was_able_to_drop = false;
                    {
                        // First, remove any existing contains links for the dropped entity..
                        let mut v_links = self.world.borrow::<ViewMut<Links>>().unwrap();

                        for (id, links) in (&mut v_links).iter().with_id() {
                            links.to_links.retain(|link| {
                                let is_link_to_entity = matches!(link.link, Link::Contains(_))
                                    && link.to_entity_id.is_some()
                                    && link.to_entity_id.unwrap().0 == dropped_entity_id;

                                !is_link_to_entity
                            });

                            // If it is the parent, we'll add the link!
                            if id == parent_entity_id {
                                links.to_links.push(ToLink {
                                    link: Link::Contains(0),
                                    to_entity_id: Some(dark::properties::WrappedEntityId(
                                        dropped_entity_id,
                                    )),
                                    to_template_id: 0, // todo?
                                });
                                was_able_to_drop = true;
                            }
                        }
                    }
                    if was_able_to_drop {
                        self.world
                            .add_component(dropped_entity_id, PropHasRefs(false));
                        self.make_un_physical(dropped_entity_id);
                    }
                }

                Effect::GrabEntity {
                    entity_id,
                    hand,
                    current_parent_id: _,
                } => {
                    if hand == vr_config::Handedness::Left {
                        self.left_hand = self.left_hand.grab_entity(&self.world, entity_id);
                    } else {
                        self.right_hand = self.right_hand.grab_entity(&self.world, entity_id);
                    }

                    if self.right_hand.is_holding(entity_id) || self.left_hand.is_holding(entity_id)
                    {
                        // Let the scripts know we are now holding the item..
                        self.script_world.dispatch(Message {
                            payload: MessagePayload::Hold,
                            to: entity_id,
                        });

                        let mut v_has_refs = self.world.borrow::<ViewMut<PropHasRefs>>().unwrap();
                        if let Ok(has_refs) = (&mut v_has_refs).get(entity_id) {
                            has_refs.0 = true;
                        }

                        let mut v_links = self.world.borrow::<ViewMut<Links>>().unwrap();

                        for links in (&mut v_links).iter() {
                            links.to_links.retain(|link| {
                                let is_link_to_entity = matches!(link.link, Link::Contains(_))
                                    && link.to_entity_id.is_some()
                                    && link.to_entity_id.unwrap().0 == entity_id;

                                !is_link_to_entity
                            })
                        }
                    }
                }
                Effect::SetJointTransform {
                    entity_id,
                    joint_id,
                    transform,
                } => {
                    let maybe_player = self.id_to_animation_player.get_mut(&entity_id);
                    if let Some(player) = maybe_player {
                        *player = AnimationPlayer::set_additional_joint_transform(
                            player, joint_id, transform,
                        )
                    }
                }

                Effect::QueueAnimationBySchema {
                    entity_id,
                    motion_query_items,
                    selection_strategy,
                } => {
                    let maybe_player = self.id_to_animation_player.get_mut(&entity_id);
                    if let Some(player) = maybe_player {
                        let v_creature_type = self.world.borrow::<View<PropCreature>>().unwrap();

                        let v_motion_actor_tag =
                            self.world.borrow::<View<PropMotionActorTags>>().unwrap();

                        if let (Ok(creature_type), Ok(motion_actor_tag)) = (
                            v_creature_type.get(entity_id),
                            v_motion_actor_tag.get(entity_id),
                        ) {
                            let mut actor_tags = motion_actor_tag
                                .tags
                                .iter()
                                .map(|tag| MotionQueryItem::new(tag).optional())
                                .collect::<Vec<MotionQueryItem>>();

                            let mut query_items = motion_query_items.clone();

                            query_items.append(&mut actor_tags);

                            let creature_definition =
                                get_creature_definition(creature_type.0).unwrap();

                            let actor_type = creature_definition.actor_type.to_u32().unwrap();

                            let query = MotionQuery::new(actor_type, query_items)
                                .with_selection_strategy(selection_strategy);
                            // let query_with_actor =
                            let maybe_next_animation = global_context.motiondb.query(query.clone());
                            if let Some(next_animation) = maybe_next_animation {
                                let maybe_clip = asset_cache.get_opt(
                                    &ANIMATION_CLIP_IMPORTER,
                                    &format!("{}_.mc", next_animation),
                                );

                                if let Some(clip) = maybe_clip {
                                    *player = AnimationPlayer::queue_animation(player, clip);
                                } else {
                                    game_log!(
                                        WARN,
                                        "Unable to load animation clip: {:?}_.mc",
                                        next_animation
                                    );
                                }
                            } else {
                                game_log!(WARN, "Unable to find animation for query: {:?}", &query);
                                // If we couldn't find an animation... just stop the current one
                                self.script_world.dispatch(Message {
                                    payload: MessagePayload::AnimationCompleted,
                                    to: entity_id,
                                });
                            }
                        }
                    }
                }

                Effect::Send { msg } => {
                    println!("handling Effect::Send event: {:?}", msg);
                    self.script_world.dispatch(msg);
                }

                Effect::SetUI {
                    parent_entity,
                    handle,
                    world_offset,
                    world_size,
                    components,
                } => {
                    if game_options.experimental_features.contains("gui") {
                        self.gui.update_ui(
                            &mut self.world,
                            &mut self.physics,
                            &mut self.script_world,
                            &mut self.id_to_physics,
                            handle,
                            parent_entity,
                            world_size,
                            world_offset,
                            components,
                        );
                    }
                }

                Effect::ReplaceEntity {
                    entity_id,
                    template_id,
                } => {
                    let (position, rotation) = {
                        if let Some(handle) = &self.id_to_physics.get(&entity_id) {
                            let position = self.physics.get_position(**handle).unwrap();
                            let rotation = self.physics.get_rotation(**handle).unwrap();
                            // let scale_xform =
                            //     Matrix4::from_nonuniform_scale(scale.x.abs(), scale.y.abs(), scale.z.abs());
                            (position, rotation)
                        } else {
                            (
                                vec3(0.0, 0.0, 0.0),
                                Quaternion {
                                    s: 1.0,
                                    v: vec3(0.0, 0.0, 0.0),
                                },
                            )
                        }
                    };

                    let new_entity_info = self.create_entity_with_position(
                        asset_cache,
                        template_id,
                        vec3_to_point3(position),
                        rotation,
                        Matrix4::identity(),
                        CreateEntityOptions::default(),
                    );

                    if new_entity_info.rigid_body.is_some() {
                        let rigid_body = new_entity_info.rigid_body.unwrap();
                        self.left_hand = self.left_hand.replace_entity(
                            entity_id,
                            new_entity_info.entity_id,
                            rigid_body,
                        );
                        self.right_hand = self.right_hand.replace_entity(
                            entity_id,
                            new_entity_info.entity_id,
                            rigid_body,
                        );
                    }

                    self.remove_entity(entity_id);
                }

                Effect::ChangeModel {
                    entity_id,
                    model_name,
                } => {
                    if let Some(model) = self.id_to_model.get(&entity_id) {
                        // if !scene_objs.is_empty() {
                        //let scene_obj = scene_objs.get(0).unwrap().borrow();
                        let xform = model.get_transform();
                        //drop(scene_obj);

                        let _ext_name = model_name.clone();
                        let orig_model =
                            asset_cache.get(&MODELS_IMPORTER, &format!("{model_name}.BIN"));

                        let orig_model_ref = orig_model.as_ref();

                        let new_model = Model::transform(orig_model_ref, xform);

                        let vhots = new_model.vhots();
                        self.id_to_model.insert(entity_id, new_model);
                        self.world
                            .add_component(entity_id, PropModelName(model_name));

                        self.world.add_component(entity_id, RuntimePropVhots(vhots));
                    }
                }
                Effect::PlayEmail { deck, email, force } => {
                    let email_file = get_email_sound_file(deck, email);
                    let mut quests = self.world.borrow::<UniqueViewMut<QuestInfo>>().unwrap();
                    let has_read = quests.has_played_email(&email_file);
                    if !has_read || force {
                        quests.mark_email_as_played(&email_file);
                        let audio_clip =
                            asset_cache.get(&AUDIO_IMPORTER, &format!("{email_file}.wav"));
                        engine::audio::test_audio(
                            audio_context,
                            AudioHandle::new(),
                            Some(AudioChannel::new("email".to_owned())),
                            audio_clip,
                        );
                    }
                    drop(quests);
                }
                Effect::PlaySound { handle, name } => {
                    println!("Trying to play sound: {}", &name);
                    let audio_file = resolve_schema(global_context, &name.to_string());
                    let maybe_audio_clip =
                        asset_cache.get_opt(&AUDIO_IMPORTER, &format!("{audio_file}.wav"));

                    if let Some(audio_clip) = maybe_audio_clip {
                        info!("Playing clip: {} handle: {:?}", name, &handle);
                        engine::audio::test_audio(audio_context, handle, None, audio_clip);
                    } else {
                        warn!("Unable to load clip: {}", name)
                    }
                }
                Effect::PlaySpeech {
                    entity_id,
                    voice_index,
                    concept,
                    tags,
                } => {
                    if let Some(sample_name) = resolve_speech_sample(
                        &global_context.gamesys,
                        voice_index,
                        concept.as_str(),
                        &tags,
                    ) {
                        let audio_path = format!("{sample_name}.wav");
                        if let Some(audio_clip) = asset_cache.get_opt(&AUDIO_IMPORTER, &audio_path)
                        {
                            let handle = AudioHandle::new();
                            if let Some(position) = get_entity_position(&self.world, entity_id) {
                                engine::audio::play_spatial_audio(
                                    audio_context,
                                    position,
                                    handle,
                                    None,
                                    audio_clip,
                                );
                            } else {
                                engine::audio::test_audio(audio_context, handle, None, audio_clip);
                            }
                        } else {
                            warn!(
                                "Unable to load speech clip '{}' for concept '{}'",
                                audio_path, concept
                            );
                        }
                    } else {
                        warn!(
                            "Failed to resolve speech for voice {} concept '{}'",
                            voice_index, concept
                        );
                    }
                }
                Effect::PlayEnvironmentalSound {
                    query,
                    position,
                    audio_handle,
                } => {
                    play_environmental_sound(
                        &global_context.gamesys,
                        asset_cache,
                        audio_context,
                        query,
                        audio_handle,
                        position,
                    );
                }
                Effect::SlayEntity { entity_id } => {
                    let did_slay = self.slay_entity(entity_id, asset_cache);

                    if did_slay {
                        let maybe_env_sound_query =
                            get_environmental_sound_query(&self.world, entity_id, "death", vec![]);

                        if let (Some(handle), Some(env_sound_query)) =
                            (self.id_to_physics.get(&entity_id), maybe_env_sound_query)
                        {
                            let position = self.physics.get_position(*handle).unwrap();

                            play_environmental_sound(
                                &global_context.gamesys,
                                asset_cache,
                                audio_context,
                                env_sound_query,
                                AudioHandle::new(),
                                position,
                            )
                        }

                        self.remove_entity(entity_id);
                    }
                }
                Effect::StopSound { handle } => {
                    engine::audio::stop_audio(audio_context, handle);
                }
                Effect::DestroyEntity { entity_id } => {
                    info!("!!!Destroying entity: {:?}", entity_id);
                    self.left_hand = self.left_hand.destroy_entity(entity_id);
                    self.right_hand = self.right_hand.destroy_entity(entity_id);
                    self.remove_entity(entity_id);
                }
                Effect::ResetGravity { entity_id } => {
                    self.physics.set_gravity(entity_id, 1.0);
                }
                Effect::SetGravity {
                    entity_id,
                    gravity_percent,
                } => {
                    self.physics.set_gravity(entity_id, gravity_percent);
                }
                Effect::SetPlayerPosition {
                    position,
                    is_teleport,
                } => {
                    self.physics
                        .set_player_translation(position, &mut self.player_handle);
                    if is_teleport {
                        self.world
                            .add_component(player_entity, PropTeleported::new())
                    }
                }
                Effect::SetQuestBit {
                    quest_bit_name,
                    quest_bit_value,
                } => {
                    let mut quests = self.world.borrow::<UniqueViewMut<QuestInfo>>().unwrap();
                    quests.set_quest_bit_value(&quest_bit_name, quest_bit_value);
                    drop(quests);

                    let quests_new = self.world.borrow::<UniqueView<QuestInfo>>().unwrap();
                    info!(
                        "Updated quest info for {}({:?}): {:?}",
                        quest_bit_name, quest_bit_value, quests_new
                    );
                }

                Effect::SetAIProperty { entity_id, update } => match update {
                    AIPropertyUpdate::Alertness { level, peak } => {
                        self.world
                            .add_component(entity_id, PropAIAlertness { level, peak });
                    }
                    AIPropertyUpdate::Mode { mode } => {
                        self.world.add_component(entity_id, PropAIMode { mode });
                    }
                },
                Effect::SetPositionRotation {
                    entity_id,
                    rotation,
                    position,
                } => {
                    // TODO: plumb scale through
                    self.set_entity_position_rotation(
                        entity_id,
                        position,
                        rotation,
                        vec3(1.0, 1.0, 1.0),
                    );
                }
                Effect::SetPosition {
                    entity_id,
                    position,
                } => {
                    self.id_to_model.entry(entity_id).and_modify(|model| {
                        let mut xform = model.get_transform();
                        xform.w.x = position.x;
                        xform.w.y = position.y;
                        xform.w.z = position.z;
                        *model = Model::transform(model, xform);
                    });

                    if let Some(rigid_body_handle) = self.id_to_physics.get(&entity_id) {
                        self.physics.set_translation(*rigid_body_handle, position);
                    };
                }
                Effect::SetRotation {
                    entity_id,
                    rotation,
                } => {
                    if let Some(rigid_body_handle) = self.id_to_physics.get(&entity_id) {
                        self.physics.set_rotation(*rigid_body_handle, rotation);
                    };
                }
                Effect::PositionInventory { position, rotation } => {
                    PlayerInventoryEntity::set_position_rotation(
                        &mut self.world,
                        position,
                        rotation,
                    )
                }
                Effect::TurnOffTweqs { entity_id } => {
                    self.world.run_with_data(turn_off_tweqs, entity_id);
                }
                Effect::TurnOnTweqs { entity_id } => {
                    self.world.run_with_data(turn_on_tweqs, entity_id);
                }
                Effect::GlobalEffect(global_effect) => global_effects.push(global_effect),
                _ => {
                    game_log!(WARN, "Unhandled effect: {effect:?}");
                }
            }
        }

        global_effects
    }
    pub fn render_per_eye(
        &mut self,
        asset_cache: &mut AssetCache,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
        screen_size: Vector2<f32>,
        options: &crate::GameOptions,
    ) -> Vec<SceneObject> {
        let mut ret = vec![];
        if let Some(hit_entity) = self.left_hand.get_raytraced_entity() {
            ret.extend(draw_item_outline(
                asset_cache,
                &self.physics,
                hit_entity,
                view,
                projection,
                screen_size,
            ));

            ret.extend(draw_item_name(
                asset_cache,
                &self.physics,
                hit_entity,
                &self.world,
                view,
                projection,
                screen_size,
                options.debug_show_ids,
            ));
        };

        if let Some(hit_entity) = self.right_hand.get_raytraced_entity() {
            ret.extend(draw_item_outline(
                asset_cache,
                &self.physics,
                hit_entity,
                view,
                projection,
                screen_size,
            ));
            ret.extend(draw_item_name(
                asset_cache,
                &self.physics,
                hit_entity,
                &self.world,
                view,
                projection,
                screen_size,
                options.debug_show_ids,
            ));
        };

        ret.extend(self.visibility_engine.debug_render(asset_cache));

        ret
    }

    pub fn finish_render(
        &mut self,
        _asset_cache: &mut AssetCache,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
        screen_size: Vector2<f32>,
    ) {
        let culling_info = CullingInfo {
            view,
            projection,
            screen_size,
        };
        profile!(
            scope: "render", level: DEBUG, "visibility_engine.prepare",
            self.visibility_engine
                .prepare(self.spatial_data.as_deref(), &self.world, &culling_info)
        );
    }

    pub fn ambient_audio_state(&self) -> Option<AmbientAudioState> {
        let player_position = {
            let player_info = self.world.borrow::<UniqueView<PlayerInfo>>().ok()?;
            player_info.pos
        };

        let Ok((v_ambient_hacked, v_position)) = self
            .world
            .borrow::<(View<PropAmbientHacked>, View<PropPosition>)>()
        else {
            return Some(AmbientAudioState {
                player_position,
                music_cue: None,
                environmental_cue: None,
                ambient_emitters: Vec::new(),
            });
        };

        let mut music_cue = None;
        let mut environmental_cue = None;
        let mut emitter_candidates: Vec<(f32, EntityId, Vector3<f32>, String)> = Vec::new();

        for (id, (ambient_sound, position)) in (&v_ambient_hacked, &v_position).iter().with_id() {
            let dist_squared = (position.position - player_position).magnitude2();

            if dist_squared < ambient_sound.radius_squared {
                if ambient_sound.sound_flags.contains(AmbientSoundFlags::MUSIC) {
                    music_cue = Some(ambient_sound.schema.clone());
                } else if ambient_sound
                    .sound_flags
                    .contains(AmbientSoundFlags::ENVIRONMENTAL)
                {
                    environmental_cue = Some(ambient_sound.schema.clone());
                } else {
                    emitter_candidates.push((
                        dist_squared,
                        id,
                        position.position,
                        ambient_sound.schema.clone(),
                    ));
                }
            }
        }

        emitter_candidates.sort_by(|a, b| a.0.total_cmp(&b.0));

        let ambient_emitters = emitter_candidates
            .into_iter()
            .take(8)
            .map(|(_, id, position, sample_name)| (id, position, sample_name))
            .collect();

        Some(AmbientAudioState {
            player_position,
            music_cue,
            environmental_cue,
            ambient_emitters,
        })
    }

    pub fn render(
        &mut self,
        asset_cache: &mut AssetCache,
        options: &GameOptions,
    ) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        let _v_position = self.world.borrow::<View<PropPosition>>().unwrap();
        let v_transform = self.world.borrow::<View<RuntimePropTransform>>().unwrap();
        let v_frame_state = self.world.borrow::<View<PropFrameAnimState>>().unwrap();
        let v_render_type = self.world.borrow::<View<PropRenderType>>().unwrap();
        let v_joint_transforms = self
            .world
            .borrow::<View<RuntimePropJointTransforms>>()
            .unwrap();

        // Start with built in scene objects
        let mut scene = self.scene_objects.clone();

        let mut total_model_count = 0;
        let mut rendered_model_count = 0;

        // Render models
        for (entity_id, objs) in &self.id_to_model {
            total_model_count += 1;
            if !has_refs(&self.world, *entity_id) {
                continue;
            }

            if v_render_type.contains(*entity_id) {
                let render_type = v_render_type.get(*entity_id).unwrap();
                if render_type.0 == RenderType::EditorOnly || render_type.0 == RenderType::NoRender
                {
                    continue;
                };
            }

            if !self.visibility_engine.is_visible(*entity_id) {
                continue;
            }

            rendered_model_count += 1;

            let scene_objs = {
                if let Some(player) = self.id_to_animation_player.get(entity_id) {
                    objs.to_animated_scene_objects(player)
                } else {
                    objs.to_scene_objects().clone()
                }
            };
            let is_animated_model = objs.is_animated();

            if let Ok(xform) = v_transform.get(*entity_id).map(|p| p.0) {
                for obj in scene_objs {
                    let mut xformed_obj = obj.clone();
                    xformed_obj.set_transform(xform);
                    if options.debug_skeletons && is_animated_model {
                        xformed_obj.set_depth_write(false);
                        xformed_obj.set_skinned_transparency(Some(0.35));
                    } else {
                        xformed_obj.set_depth_write(true);
                        xformed_obj.set_skinned_transparency(None);
                    }
                    scene.push(xformed_obj);
                }

                if options.debug_skeletons {
                    if let Ok(joint_transforms) = v_joint_transforms.get(*entity_id) {
                        let world_joints: Vec<Matrix4<f32>> = joint_transforms
                            .0
                            .iter()
                            .map(|joint| xform * *joint)
                            .collect();
                        let mut debug_skeleton = objs.draw_debug_skeleton(&world_joints);
                        scene.append(&mut debug_skeleton);
                    }
                }
            }
        }
        game_log!(
            TRACE,
            "Rendered models: {} / {} total",
            rendered_model_count,
            total_model_count
        );
        // Render bitmap_animation
        for (entity_id, objs) in &self.id_to_bitmap {
            if !self.visibility_engine.is_visible(*entity_id) {
                continue;
            }

            if let Ok(xform) = v_transform.get(*entity_id).map(|p| p.0) {
                let current_frame = v_frame_state
                    .get(*entity_id)
                    .map(|c| c.current_frame)
                    .unwrap_or(0);
                let texture: Rc<dyn TextureTrait> = objs
                    .get_frame(current_frame as usize, dark::FrameOptions::Wrap)
                    .unwrap();
                let mat = BillboardMaterial::create(texture, 1.0, 0.0, 1.0);
                let mut scene_obj = SceneObject::new(mat, Box::new(quad::create()));
                scene_obj.set_transform(xform);
                scene.push(scene_obj);
            }
        }
        // Render particle systems
        if options.render_particles {
            for (particle_entity_id, particle_system) in &self.id_to_particle_system {
                if !self.visibility_engine.is_visible(*particle_entity_id) {
                    continue;
                }

                let particle_systems = particle_system.render();
                scene.extend(particle_systems);
            }
        }

        // Render player
        let player = self.world.borrow::<UniqueView<PlayerInfo>>().unwrap();

        let player_mat = engine::scene::color_material::create(Vector3::new(0.0, 0.0, 1.0));
        let mut _player = SceneObject::new(player_mat, Box::new(engine::scene::cube::create()));
        _player.set_transform(Matrix4::from_translation(player.pos));

        // Render hands
        scene.append(&mut self.left_hand.render());
        scene.append(&mut self.right_hand.render());

        // Render forearm HUD panels with health/psi overlays
        let mut hud_panels = crate::hud::create_arm_hud_panels(
            asset_cache,
            &self.world,
            self.left_hand.get_position(),
            self.left_hand.get_rotation(),
            self.right_hand.get_position(),
            self.right_hand.get_rotation(),
        );
        scene.append(&mut hud_panels);

        // Render inventory
        let inventory_objs = PlayerInventoryEntity::render(&self.world);
        scene.extend(inventory_objs);

        // Render debug physics
        if options.debug_physics {
            let debug_render = &self.physics.debug_render();
            scene.append(&mut debug_render.clone());
        }

        // self.world.run(
        //     |v_position: View<PropPosition>,
        //      v_sym_name: View<PropSymName>,
        //      v_runtime_transform: View<RuntimePropTransform>| {
        //         for (_id, (_position, sym_name, transform)) in
        //             (&v_position, &v_sym_name, &v_runtime_transform)
        //                 .iter()
        //                 .with_id()
        //         {
        //             let font = asset_cache.get(&FONT_IMPORTER, "mainfont.fon").clone();

        //             let position = transform.0.transform_point(point3(0.0, 0.0, 0.0));
        //             let mut text = SceneObject::world_space_text(&sym_name.0, font, 0.0);
        //             text.set_transform(
        //                 Matrix4::from_translation(point3_to_vec3(position))
        //                     * Matrix4::from(input_context.head.rotation.invert())
        //                     * Matrix4::from_angle_y(Rad(std::f32::consts::PI / -2.0)),
        //             );
        //             scene.push(text);
        //         }
        //     },
        // );

        // Render debug lines

        for line in &self.debug_lines {
            let start = line.start;
            let end = line.end;
            let color = line.color;
            let lines_mat =
                engine::scene::color_material::create(Vector3::new(color.x, color.y, color.z));
            let vertices = vec![
                VertexPosition {
                    position: start.to_vec(),
                },
                VertexPosition {
                    position: end.to_vec(),
                },
            ];
            let debug = SceneObject::new(
                lines_mat,
                Box::new(engine::scene::lines_mesh::create(vertices)),
            );
            scene.push(debug);
        }

        // Render gui
        if options.experimental_features.contains("gui") {
            let guis = self.gui.render(asset_cache, &self.world);

            scene.extend(guis);
        }

        // Note: Hand spotlights for enhanced lighting are now handled in the runtime
        // via get_hand_spotlights() method - they're added to the Scene's lighting system

        if options.debug_portals {
            if let Some(spatial_data) = &self.spatial_data {
                let (player_pos, _player_rot) = {
                    let player_info = self.world.borrow::<UniqueView<PlayerInfo>>().unwrap();
                    (player_info.pos, player_info.rotation)
                };
                let maybe_cell = spatial_data.get_cell_from_position(player_pos);
                if let Some(cell) = maybe_cell {
                    // println!(
                    //     "!! pos: {:?}  [cell-idx]: {:?} center: {:?} radius: {:?}",
                    //     player_pos, cell.idx, cell.center, cell.radius
                    // );
                    scene.extend(cell.debug_render());
                } else {
                    game_log!(WARN, "Unable to find cell at position: {:?}", player_pos);
                }
            }
        }

        (scene, player.pos, player.rotation)
    }

    /// Get hand spotlights for testing enhanced lighting system
    /// Returns a vector of SpotLight objects positioned at the player's hands
    pub fn get_hand_spotlights(&self, options: &GameOptions) -> Vec<SpotLight> {
        let mut lights = Vec::new();

        if options.experimental_features.contains("enhanced_lighting") {
            // Right hand spotlight
            let right_hand_pos = self.right_hand.get_position();
            let right_hand_rot = self.right_hand.get_rotation();

            // Convert quaternion to direction vector (forward direction)
            let right_direction = right_hand_rot * Vector3::new(0.0, 0.0, -1.0);

            let right_spotlight = SpotLight {
                position: right_hand_pos,
                direction: right_direction.normalize(),
                color_intensity: cgmath::Vector4::new(1.0, 1.0, 0.8, 2.0), // Warm white, intensity 2.0
                inner_cone_angle: 15.0_f32.to_radians(),                   // 15 degree inner cone
                outer_cone_angle: 30.0_f32.to_radians(),                   // 30 degree outer cone
                range: 10.0,                                               // 10 meter range
            };
            lights.push(right_spotlight);

            // Left hand spotlight
            let left_hand_pos = self.left_hand.get_position();
            let left_hand_rot = self.left_hand.get_rotation();

            // Convert quaternion to direction vector (forward direction)
            let left_direction = left_hand_rot * Vector3::new(0.0, 0.0, -1.0);

            let left_spotlight = SpotLight {
                position: left_hand_pos,
                direction: left_direction.normalize(),
                color_intensity: cgmath::Vector4::new(1.0, 1.0, 0.8, 2.0), // Warm white, intensity 2.0
                inner_cone_angle: 15.0_f32.to_radians(),                   // 15 degree inner cone
                outer_cone_angle: 30.0_f32.to_radians(),                   // 30 degree outer cone
                range: 10.0,                                               // 10 meter range
            };
            lights.push(left_spotlight);
        }

        lights
    }

    fn update_avatar_hands(
        &mut self,
        asset_cache: &mut AssetCache,
        player_pos: Vector3<f32>,
        player_rotation: Quaternion<f32>,
        input_context: &input_context::InputContext,
    ) {
        let (right_hand, mut right_hand_msgs) = VirtualHand::update(
            &self.right_hand,
            &self.physics,
            &self.world,
            player_pos,
            player_rotation,
            &input_context.right_hand,
        );
        self.right_hand = right_hand;

        let (left_hand, mut left_hand_msgs) = VirtualHand::update(
            &self.left_hand,
            &self.physics,
            &self.world,
            player_pos,
            player_rotation,
            &input_context.left_hand,
        );
        self.left_hand = left_hand;

        left_hand_msgs.append(&mut right_hand_msgs);

        for msg in left_hand_msgs {
            match msg {
                VirtualHandEffect::OutMessage { message } => self.script_world.dispatch(message),
                VirtualHandEffect::ApplyForce {
                    entity_id,
                    force,
                    torque,
                } => {
                    if let Some(rigid_body_handle) = self.id_to_physics.get(&entity_id) {
                        self.physics.apply_torque(*rigid_body_handle, torque);
                        self.physics.apply_force(*rigid_body_handle, force)
                    };
                }
                VirtualHandEffect::SetPositionRotation {
                    entity_id,
                    position,
                    rotation,
                    scale,
                } => {
                    self.set_entity_position_rotation(entity_id, position, rotation, scale);
                }
                VirtualHandEffect::SpawnEntity {
                    template_id,
                    position,
                    rotation,
                } => {
                    self.create_entity_with_position(
                        asset_cache,
                        template_id,
                        vec3_to_point3(position),
                        rotation,
                        Matrix4::identity(),
                        CreateEntityOptions::default(),
                    );
                }
                VirtualHandEffect::HoldItem { entity_id } => {
                    self.make_un_physical(entity_id);
                    self.script_world.dispatch(Message {
                        payload: MessagePayload::Hold,
                        to: entity_id,
                    });
                }
                VirtualHandEffect::DropItem { entity_id } => {
                    self.make_physical(entity_id);

                    self.script_world.dispatch(Message {
                        payload: MessagePayload::Drop,
                        to: entity_id,
                    });
                }
            }
        }
    }

    /// Queue an entity to be triggered after scripts are initialized
    pub fn queue_entity_trigger(&mut self, entity_name: String) {
        println!("Queueing entity trigger for: {}", entity_name);
        self.pending_entity_triggers.push(entity_name);
    }

    /// Internal method to trigger an entity and return messages to dispatch
    fn trigger_entity_by_name_internal(&mut self, entity_name: String) -> Vec<Message> {
        let entities = scripts::script_util::get_entities_by_name(&self.world, &entity_name);
        println!(
            "Triggering {} entities with name: {}",
            entities.len(),
            entity_name
        );

        let mut messages = Vec::new();
        for entity_id in entities {
            // Get all switch links and create TurnOn messages for each target
            let switch_links = scripts::script_util::get_all_switch_links(&self.world, entity_id);
            println!(
                "Found {} switch links for entity {:?}",
                switch_links.len(),
                entity_id
            );

            for target_entity_id in switch_links {
                let message = Message {
                    payload: MessagePayload::TurnOn { from: entity_id },
                    to: target_entity_id,
                };
                println!(
                    "Creating TurnOn message from {:?} to {:?}",
                    entity_id, target_entity_id
                );
                messages.push(message);
            }
        }
        messages
    }
}

fn create_template_name_map(game_entity_info: &Gamesys) -> HashMap<String, EntityMetadata> {
    let mut gamesys_world = World::new();
    game_entity_info.entity_info.initialize_world_with_entities(
        &mut gamesys_world,
        HashMap::new(),
        |_id| true,
    );

    let mut name_to_template_id = HashMap::new();

    gamesys_world.run(
        |v_sym_name: View<dark::properties::PropSymName>,
         v_obj_icon: View<dark::properties::PropObjIcon>,
         v_obj_short_name: View<dark::properties::PropObjShortName>,
         v_obj_name: View<dark::properties::PropObjName>,
         v_template_id: View<dark::properties::PropTemplateId>| {
            for (entity_id, (sym_name, template_id)) in
                (&v_sym_name, &v_template_id).iter().with_id()
            {
                name_to_template_id.insert(
                    sym_name.0.to_ascii_lowercase(),
                    EntityMetadata {
                        template_id: template_id.template_id,
                        obj_icon: v_obj_icon
                            .get(entity_id)
                            .map(|p| format!("{}.pcx", p.0))
                            .ok(),
                        obj_name: v_obj_name.get(entity_id).map(|p| p.0.clone()).ok(),
                        obj_short_name: v_obj_short_name.get(entity_id).map(|p| p.0.clone()).ok(),
                    },
                );
            }
        },
    );

    name_to_template_id
}

///
/// initialize_background_music
///
/// Helper function to set up the music player for the level
fn initialize_background_music(
    song_params: &SongParams,
    asset_cache: &mut AssetCache,
    audio_context: &mut AudioContext<EntityId, String>,
) {
    let song_file_name = &song_params.song;
    info!("loading music for level: {}", song_file_name);
    if !song_file_name.is_empty() {
        let song = {
            asset_cache
                .get(&SONG_IMPORTER, &format!("{song_file_name}.snc"))
                .clone()
        };
        let background_music_player = SongPlayer::new(&song, asset_cache);
        audio_context.set_background_music(Box::new(background_music_player));
    } else {
        audio_context.stop_background_music();
    }
}

fn create_room_entities(
    room_db: &RoomDatabase,
    template_to_entity_id: &HashMap<i32, WrappedEntityId>,
    world: &mut World,
    entities_to_initialize: &mut HashSet<(EntityId, i32)>,
) {
    // HACK: The collision detection for entering / exiting rooms is different here
    // than in Dark. We fire the edge detection events on any intersection, whereas
    // Dark seems to use a stricter check. For now, we'll just offset the rooms
    // to give a similar effect....
    let vert_offset = vec3(0.0, 5.0 / SCALE_FACTOR, 0.0);

    for room in &room_db.rooms {
        let link = Links {
            to_links: option_to_vec(template_to_entity_id.get(&room.obj_id).map(|id| ToLink {
                link: Link::SwitchLink,
                to_entity_id: Some(*id),
                to_template_id: room.obj_id,
            })),
        };

        let _room = world.add_entity((
            RuntimePropDoNotSerialize,
            PropPosition {
                position: room.center + vert_offset,
                rotation: Quaternion {
                    v: Vector3::zero(),
                    s: 1.0,
                },
                cell: 0, // TODO - needed?
            },
            PropScripts {
                scripts: vec!["internal_room_trigger".to_owned()],
                inherits: false,
            },
            PropPhysDimensions {
                radius0: 0.0,
                radius1: 1.0,
                unk1: 0,
                unk2: 0,
                offset0: Vector3::zero(),
                offset1: Vector3::zero(),
                size: (room.bounding_box.max - room.bounding_box.min),
            },
            PropPhysType {
                is_special: false,
                num_submodels: 1,
                phys_type: PhysicsModelType::ORIENTED_BOUNDING_BOX,
                remove_on_sleep: false,
            },
            PropTripFlags {
                trip_flags: TripFlags::ENTER | TripFlags::EXIT | TripFlags::PLAYER,
            },
            PropPhysState {
                position: room.center + vert_offset,
                velocity: Vector3::zero(),
                rot_velocity: Vector3::zero(),
                rotation: Quaternion {
                    v: Vector3::zero(),
                    s: 1.0,
                },
            },
            link,
        ));
        entities_to_initialize.insert((_room, room.obj_id));
    }
}

fn option_to_vec<T>(option: Option<T>) -> Vec<T> {
    match option {
        None => vec![],
        Some(v) => vec![v],
    }
}

pub fn make_un_physical2(
    id_to_physics: &mut HashMap<EntityId, RigidBodyHandle>,
    physics: &mut PhysicsWorld,
    entity_id: EntityId,
) {
    let current_entity = id_to_physics.get(&entity_id);
    if current_entity.is_none() {
        return;
    }

    physics.remove(entity_id);
    id_to_physics.remove(&entity_id);
}

fn get_entity_position(world: &World, entity_id: EntityId) -> Option<Vector3<f32>> {
    if let Ok(positions) = world.borrow::<View<PropPosition>>() {
        if let Ok(prop) = positions.get(entity_id) {
            return Some(prop.position);
        }
    }
    None
}

fn resolve_schema(global_context: &GlobalContext, name: &str) -> String {
    let sound_schema = &global_context.gamesys.sound_schema;
    let ret = sound_schema
        .get_random_sample(name)
        .unwrap_or_else(|| name.to_owned());
    trace!("resolved sound schema {} to {}", name, ret);
    ret
}

fn resolve_speech_sample(
    gamesys: &Gamesys,
    voice_index: usize,
    concept: &str,
    tags: &[(String, String)],
) -> Option<String> {
    let speech_db = gamesys.speech_db();
    if voice_index >= speech_db.voices.len() {
        return None;
    }

    let concept_key = concept.to_ascii_lowercase();
    let concept_idx = speech_db.concept_map.get_index(&concept_key)? as usize;

    let voice = &speech_db.voices[voice_index];
    if concept_idx >= voice.tag_maps.len() {
        return None;
    }

    let tag_db = &voice.tag_maps[concept_idx];

    let mut query_items = Vec::new();
    for (tag, value) in tags {
        let lowered_tag = tag.to_ascii_lowercase();
        let lowered_value = value.to_ascii_lowercase();
        let tag_idx = speech_db.tag_map.get_index(&lowered_tag);
        let value_idx = speech_db
            .value_map
            .get_index(&lowered_value)
            .map(|idx| idx as u8);

        if let (Some(tag_id), Some(value_id)) = (tag_idx, value_idx) {
            query_items.push(TagQueryItem::KeyWithEnumValue(tag_id, value_id, false));
        }
    }

    let schema_candidates = if query_items.is_empty() {
        tag_db.collect_all_data_ids()
    } else {
        let query = TagQuery::from_items(query_items);
        tag_db.query_match_all(&query)
    };

    if schema_candidates.is_empty() {
        return None;
    }

    let mut rng = thread_rng();
    let schema_id = *schema_candidates
        .choose(&mut rng)
        .unwrap_or(&schema_candidates[0]);

    let samples = gamesys.sound_schema.id_to_samples.get(&schema_id)?;
    if samples.is_empty() {
        return None;
    }

    let weights: Vec<f64> = samples
        .iter()
        .map(|sample| f64::from(sample.frequency.max(1)))
        .collect();

    let selected_index = WeightedIndex::new(weights)
        .map(|dist| dist.sample(&mut rng))
        .unwrap_or_else(|_| rng.gen_range(0..samples.len()));

    Some(samples[selected_index].sample_name.clone())
}

fn play_environmental_sound(
    gamesys: &Gamesys,
    asset_cache: &mut AssetCache,
    audio_context: &mut AudioContext<EntityId, String>,
    query: dark::EnvSoundQuery,
    audio_handle: AudioHandle,
    position: Vector3<f32>,
) {
    let maybe_audio_file = gamesys.get_random_environmental_sound(&query);
    if maybe_audio_file.is_some() {
        let audio_file = maybe_audio_file.unwrap();
        let audio_clip = asset_cache.get(&AUDIO_IMPORTER, &format!("{audio_file}.wav").to_owned());

        info!(
            "Playing clip: {} handle: {:?} position: {:?}",
            audio_file, &audio_handle, position
        );
        engine::audio::play_spatial_audio(audio_context, position, audio_handle, None, audio_clip);
    }
}

// Implementation of GameScene trait for Mission
// ============================================================================
// DebuggableScene Implementation for MissionCore
// ============================================================================

impl crate::game_scene::DebuggableScene for MissionCore {
    fn list_entities(
        &self,
        limit: Option<usize>,
        filter: Option<&str>,
    ) -> Vec<crate::game_scene::DebugEntitySummary> {
        use crate::game_scene::DebugEntitySummary;
        use shipyard::*;

        let player_pos = self.player_position();
        let mut entities = Vec::new();

        // Query all entities with position
        self.world.run(
            |_entities_iter: EntitiesView,
             v_pos: View<dark::properties::PropPosition>,
             v_sym_name: View<dark::properties::PropSymName>,
             v_scripts: View<dark::properties::PropScripts>,
             v_links: View<dark::properties::Links>| {
                for (entity_id, pos) in v_pos.iter().with_id() {
                    let name = v_sym_name
                        .get(entity_id)
                        .map(|s| s.0.clone())
                        .unwrap_or_else(|_| format!("Entity_{}", entity_id.inner()));

                    // Apply filter if provided
                    if let Some(filter_str) = filter {
                        if !wildcard_match(&name, filter_str) {
                            continue;
                        }
                    }

                    let position = [pos.position.x, pos.position.y, pos.position.z];
                    let distance = (cgmath::Vector3::from(position) - player_pos).magnitude();

                    let script_count = v_scripts
                        .get(entity_id)
                        .map(|scripts| scripts.scripts.len())
                        .unwrap_or(0);

                    let link_count = v_links
                        .get(entity_id)
                        .map(|links| links.to_links.len())
                        .unwrap_or(0);

                    // Get template ID from entity ID (negative for templates, positive for instances)
                    let template_id = entity_id.inner() as i32;

                    entities.push(DebugEntitySummary {
                        id: entity_id.inner() as i32,
                        name,
                        template_id,
                        position,
                        distance,
                        script_count,
                        link_count,
                    });
                }
            },
        );

        // Sort by distance from player
        entities.sort_by(|a, b| {
            a.distance
                .partial_cmp(&b.distance)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Apply limit if provided
        if let Some(limit) = limit {
            entities.truncate(limit);
        }

        entities
    }

    fn entity_detail(&self, id: EntityId) -> Option<crate::game_scene::DebugEntityDetail> {
        use crate::game_scene::{DebugEntityDetail, DebugLinkInfo, DebugPropertyInfo};
        use shipyard::*;

        self.world.run(
            |v_pos: View<dark::properties::PropPosition>,
             v_sym_name: View<dark::properties::PropSymName>,
             v_scripts: View<dark::properties::PropScripts>,
             v_links: View<dark::properties::Links>| {
                let position = v_pos.get(id).ok()?;
                let rotation_array = [
                    position.rotation.v.x,
                    position.rotation.v.y,
                    position.rotation.v.z,
                    position.rotation.s,
                ];

                let name = v_sym_name
                    .get(id)
                    .map(|s| s.0.clone())
                    .unwrap_or_else(|_| format!("Entity_{}", id.inner()));

                let template_id = id.inner() as i32;

                // Build properties list
                let mut properties = Vec::new();

                // Add position
                properties.push(DebugPropertyInfo {
                    name: "Position".to_string(),
                    value: format!(
                        "[{:.2}, {:.2}, {:.2}]",
                        position.position.x, position.position.y, position.position.z
                    ),
                });

                // Add rotation
                properties.push(DebugPropertyInfo {
                    name: "Rotation".to_string(),
                    value: format!(
                        "[{:.3}, {:.3}, {:.3}, {:.3}]",
                        rotation_array[0], rotation_array[1], rotation_array[2], rotation_array[3]
                    ),
                });

                // Add scripts
                if let Ok(scripts) = v_scripts.get(id) {
                    properties.push(DebugPropertyInfo {
                        name: "Scripts".to_string(),
                        value: scripts.scripts.join(", "),
                    });
                }

                // Build links
                let mut outgoing_links = Vec::new();
                let incoming_links = Vec::new();

                if let Ok(links) = v_links.get(id) {
                    for link in &links.to_links {
                        if let Some(target_entity) = link.to_entity_id {
                            outgoing_links.push(DebugLinkInfo {
                                link_type: format!("{:?}", link.link),
                                target_id: target_entity.0.inner() as i32,
                                target_name: v_sym_name
                                    .get(target_entity.0)
                                    .map(|s| s.0.clone())
                                    .unwrap_or_else(|_| {
                                        format!("Entity_{}", target_entity.0.inner())
                                    }),
                            });
                        }
                    }
                    // TODO: Incoming links require scanning all entities - simplified for now
                }

                Some(DebugEntityDetail {
                    entity_id: id.inner() as i32,
                    name,
                    template_id,
                    position: [
                        position.position.x,
                        position.position.y,
                        position.position.z,
                    ],
                    rotation: rotation_array,
                    inheritance_chain: vec![], // TODO: Implement inheritance chain lookup
                    properties,
                    outgoing_links,
                    incoming_links,
                })
            },
        )
    }

    fn raycast(
        &self,
        start: cgmath::Point3<f32>,
        end: cgmath::Point3<f32>,
        mask: crate::game_scene::RaycastMask,
    ) -> crate::game_scene::DebugRayHit {
        use crate::game_scene::DebugRayHit;

        // Convert mask to collision groups
        let collision_groups = if mask.groups.contains(&"all".to_string()) {
            crate::physics::InternalCollisionGroups::ALL
        } else {
            let mut groups = crate::physics::InternalCollisionGroups::empty();
            for group_name in &mask.groups {
                match group_name.as_str() {
                    "world" => groups |= crate::physics::InternalCollisionGroups::WORLD,
                    "entity" => groups |= crate::physics::InternalCollisionGroups::ENTITY,
                    "selectable" => groups |= crate::physics::InternalCollisionGroups::SELECTABLE,
                    "player" => groups |= crate::physics::InternalCollisionGroups::PLAYER,
                    "ui" => groups |= crate::physics::InternalCollisionGroups::UI,
                    "hitbox" => groups |= crate::physics::InternalCollisionGroups::HITBOX,
                    "raycast" => groups |= crate::physics::InternalCollisionGroups::RAYCAST,
                    _ => {} // Ignore unknown groups
                }
            }
            groups
        };

        // Perform raycast using existing physics system
        match self
            .physics
            .ray_cast3(start, end, collision_groups, None, false)
        {
            Some(hit) => {
                let entity_name = hit.maybe_entity_id.and_then(|id| {
                    self.world.run(
                        |v_sym_name: shipyard::View<dark::properties::PropSymName>| {
                            v_sym_name.get(id).ok().map(|s| s.0.clone())
                        },
                    )
                });

                DebugRayHit {
                    hit: true,
                    hit_point: Some([hit.hit_point.x, hit.hit_point.y, hit.hit_point.z]),
                    hit_normal: Some([hit.hit_normal.x, hit.hit_normal.y, hit.hit_normal.z]),
                    distance: Some((end - start).magnitude()),
                    entity_id: hit.maybe_entity_id.map(|id| id.inner() as i32),
                    entity_name,
                    collision_group: None, // TODO: Add collision group info
                    is_sensor: hit.is_sensor,
                }
            }
            None => DebugRayHit {
                hit: false,
                hit_point: None,
                hit_normal: None,
                distance: None,
                entity_id: None,
                entity_name: None,
                collision_group: None,
                is_sensor: false,
            },
        }
    }

    fn teleport_player(&mut self, position: cgmath::Vector3<f32>) -> Result<(), String> {
        // Apply the teleportation by directly updating player position
        self.world
            .run(|mut player_info: shipyard::UniqueViewMut<PlayerInfo>| {
                player_info.pos = position;
            });

        Ok(())
    }

    fn player_position(&self) -> cgmath::Vector3<f32> {
        // Get player position from PlayerInfo unique component
        self.world
            .run(|player_info: shipyard::UniqueView<PlayerInfo>| player_info.pos)
    }
}

// Helper function for wildcard matching
fn wildcard_match(text: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    let text = text.to_lowercase();
    let pattern = pattern.to_lowercase();

    if pattern.starts_with('*') && pattern.ends_with('*') {
        let inner = &pattern[1..pattern.len() - 1];
        text.contains(inner)
    } else if pattern.starts_with('*') {
        let suffix = &pattern[1..];
        text.ends_with(suffix)
    } else if pattern.ends_with('*') {
        let prefix = &pattern[..pattern.len() - 1];
        text.starts_with(prefix)
    } else {
        text.contains(&pattern)
    }
}

impl crate::game_scene::GameScene for MissionCore {
    fn update(
        &mut self,
        time: &Time,
        input_context: &InputContext,
        asset_cache: &mut AssetCache,
        game_options: &GameOptions,
        command_effects: Vec<Effect>,
    ) -> Vec<Effect> {
        self.update(
            time,
            asset_cache,
            input_context,
            game_options,
            command_effects,
        )
    }

    fn render(
        &mut self,
        asset_cache: &mut AssetCache,
        options: &GameOptions,
    ) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        self.render(asset_cache, options)
    }

    fn render_per_eye(
        &mut self,
        asset_cache: &mut AssetCache,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
        screen_size: Vector2<f32>,
        options: &GameOptions,
    ) -> Vec<SceneObject> {
        self.render_per_eye(asset_cache, view, projection, screen_size, options)
    }

    fn finish_render(
        &mut self,
        asset_cache: &mut AssetCache,
        view: Matrix4<f32>,
        projection: Matrix4<f32>,
        screen_size: Vector2<f32>,
    ) {
        self.finish_render(asset_cache, view, projection, screen_size)
    }

    fn handle_effects(
        &mut self,
        effects: Vec<Effect>,
        global_context: &GlobalContext,
        game_options: &GameOptions,
        asset_cache: &mut AssetCache,
        audio_context: &mut AudioContext<EntityId, String>,
    ) -> Vec<GlobalEffect> {
        self.handle_effects(
            effects,
            global_context,
            game_options,
            asset_cache,
            audio_context,
        )
    }

    fn get_hand_spotlights(&self, options: &GameOptions) -> Vec<SpotLight> {
        self.get_hand_spotlights(options)
    }

    fn world(&self) -> &World {
        &self.world
    }

    fn scene_name(&self) -> &str {
        &self.level_name
    }

    fn ambient_audio_state(&self) -> Option<AmbientAudioState> {
        self.ambient_audio_state()
    }

    fn queue_entity_trigger(&mut self, entity_name: String) {
        self.queue_entity_trigger(entity_name);
    }
}
