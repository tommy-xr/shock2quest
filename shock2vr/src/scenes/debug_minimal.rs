use std::collections::HashMap;

use cgmath::{InnerSpace, Matrix3, Matrix4, Quaternion, Vector3, vec3};
use dark::SCALE_FACTOR;
use engine::{
    assets::asset_cache::AssetCache,
    scene::{SceneObject, VertexPosition, light::SpotLight},
};
use shipyard::{UniqueViewMut, World};

use crate::{
    GameOptions,
    game_scene::GameScene,
    input_context::InputContext,
    inventory::PlayerInventoryEntity,
    mission::{GlobalEntityMetadata, GlobalTemplateIdMap, PlayerInfo},
    quest_info::QuestInfo,
    scripts::Effect,
    time::Time,
};

/// Minimal debug scene that keeps the player anchored and renders a single cube
pub struct DebugMinimalScene {
    world: World,
    head_rotation: Quaternion<f32>,
    player_position: Vector3<f32>,
    player_rotation: Quaternion<f32>,
    head_height: f32,
    cube_distance: f32,
    cube_scale: f32,
    hand_scale: f32,
    left_hand_position: Vector3<f32>,
    left_hand_rotation: Quaternion<f32>,
    right_hand_position: Vector3<f32>,
    right_hand_rotation: Quaternion<f32>,
    scene_name: String,
}

impl DebugMinimalScene {
    pub fn new() -> Self {
        let mut world = World::new();

        let player_entity = world.add_entity(());
        let inventory_entity = PlayerInventoryEntity::create(&mut world);
        PlayerInventoryEntity::set_position_rotation(
            &mut world,
            vec3(0.0, -1000.0, 0.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
        );

        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player_entity,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory_entity,
        });

        world.add_unique(QuestInfo::new());
        world.add_unique(GlobalTemplateIdMap(HashMap::new()));
        world.add_unique(GlobalEntityMetadata(HashMap::new()));
        world.add_unique(Time::default());

        Self {
            world,
            head_rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            player_position: vec3(0.0, 0.0, 0.0),
            player_rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            head_height: 4.0 / SCALE_FACTOR,
            cube_distance: 4.0 / SCALE_FACTOR,
            cube_scale: 0.35,
            hand_scale: 0.2,
            left_hand_position: vec3(0.0, 0.0, 0.0),
            left_hand_rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            right_hand_position: vec3(0.0, 0.0, 0.0),
            right_hand_rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            scene_name: "debug_minimal".to_owned(),
        }
    }

    fn head_base(&self) -> Vector3<f32> {
        self.player_position + vec3(0.0, self.head_height, 0.0)
    }

    fn update_player_info(&mut self) {
        if let Ok(mut player_info) = self.world.borrow::<UniqueViewMut<PlayerInfo>>() {
            player_info.pos = self.player_position;
            player_info.rotation = self.player_rotation;
        }
    }

    fn cube_object(&self) -> SceneObject {
        let color = engine::scene::color_material::create(vec3(0.2, 0.8, 1.0));
        let mut cube = SceneObject::new(color, Box::new(engine::scene::cube::create()));

        let forward = self.head_rotation * vec3(0.0, 0.0, -1.0);
        let base = self.head_base();
        let cube_position = base + forward * self.cube_distance;

        let mut look_dir = base - cube_position;
        if look_dir.magnitude2() < 1e-6 {
            look_dir = vec3(0.0, 0.0, 1.0);
        } else {
            look_dir = look_dir.normalize();
        }

        let mut up = vec3(0.0, 1.0, 0.0);
        let mut right = look_dir.cross(up);
        if right.magnitude2() < 1e-6 {
            up = vec3(0.0, 0.0, 1.0);
            right = look_dir.cross(up);
        }
        right = right.normalize();
        let true_up = right.cross(look_dir).normalize();
        let rotation_matrix = Matrix3::from_cols(right, true_up, -look_dir);
        let rotation = Quaternion::from(rotation_matrix);

        let transform = Matrix4::from_translation(cube_position)
            * Matrix4::from(rotation)
            * Matrix4::from_scale(self.cube_scale);
        cube.set_transform(transform);
        cube
    }

    fn hand_marker(
        &self,
        position: Vector3<f32>,
        rotation: Quaternion<f32>,
        color: Vector3<f32>,
    ) -> SceneObject {
        let material = engine::scene::color_material::create(color);
        let mut cube = SceneObject::new(material, Box::new(engine::scene::cube::create()));
        let transform = Matrix4::from_translation(position)
            * Matrix4::from(rotation)
            * Matrix4::from_scale(self.hand_scale);
        cube.set_transform(transform);
        cube
    }

    fn arm_segment(&self, hand_position: Vector3<f32>, color: Vector3<f32>) -> SceneObject {
        let base = self.head_base();
        let material = engine::scene::color_material::create(color);
        let vertices = vec![
            VertexPosition { position: base },
            VertexPosition {
                position: hand_position,
            },
        ];
        SceneObject::new(
            material,
            Box::new(engine::scene::lines_mesh::create(vertices)),
        )
    }

    fn hand_objects(&self) -> Vec<SceneObject> {
        let mut objs = Vec::new();
        let left_color = vec3(0.9, 0.3, 0.3);
        let right_color = vec3(0.3, 0.9, 0.3);

        objs.push(self.hand_marker(self.left_hand_position, self.left_hand_rotation, left_color));
        objs.push(self.arm_segment(self.left_hand_position, left_color));
        objs.push(self.hand_marker(
            self.right_hand_position,
            self.right_hand_rotation,
            right_color,
        ));
        objs.push(self.arm_segment(self.right_hand_position, right_color));

        objs
    }
}

impl Default for DebugMinimalScene {
    fn default() -> Self {
        Self::new()
    }
}

impl GameScene for DebugMinimalScene {
    fn update(
        &mut self,
        time: &Time,
        input_context: &InputContext,
        _asset_cache: &mut AssetCache,
        _game_options: &GameOptions,
        command_effects: Vec<Effect>,
    ) -> Vec<Effect> {
        let _ = command_effects;

        if let Ok(mut world_time) = self.world.borrow::<UniqueViewMut<Time>>() {
            *world_time = time.clone();
        }

        self.head_rotation = input_context.head.rotation;
        self.left_hand_position = input_context.left_hand.position;
        self.left_hand_rotation = input_context.left_hand.rotation;
        self.right_hand_position = input_context.right_hand.position;
        self.right_hand_rotation = input_context.right_hand.rotation;
        self.update_player_info();

        Vec::new()
    }

    fn render(
        &mut self,
        _asset_cache: &mut AssetCache,
        _options: &GameOptions,
    ) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        let mut scene = vec![self.cube_object()];
        scene.extend(self.hand_objects());
        (scene, self.player_position, self.player_rotation)
    }

    fn get_hand_spotlights(&self, _options: &GameOptions) -> Vec<SpotLight> {
        Vec::new()
    }

    fn world(&self) -> &World {
        &self.world
    }

    fn scene_name(&self) -> &str {
        &self.scene_name
    }
}
