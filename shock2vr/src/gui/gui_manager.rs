use std::collections::HashMap;

use cgmath::{EuclideanSpace, Matrix4, Vector2, Vector3, vec3};

use engine::{assets::asset_cache::AssetCache, scene::SceneObject};
use rapier3d::prelude::RigidBodyHandle;
use shipyard::{Component, EntityId, Get, View, World};

use crate::{
    physics::{CollisionGroup, PhysicsWorld},
    runtime_props::{RuntimePropDoNotSerialize, RuntimePropTransform},
    scripts::ScriptWorld,
    util::{get_position_from_transform, get_rotation_from_transform, log_entity},
};

use crate::gui::*;

pub struct GuiInstanceInfo {
    #[allow(dead_code)]
    pub parent_entity: EntityId,
    pub proxy_entity: EntityId,
    #[allow(dead_code)]
    pub offset: Vector3<f32>,
    pub components: Vec<GuiComponentRenderInfo>,
    pub world_size: Vector2<f32>,
    pub physics_handle: RigidBodyHandle,
}

pub struct GuiManager {
    handle_to_instance: HashMap<GuiHandle, GuiInstanceInfo>,
    entity_id_to_proxy_entity_id: HashMap<EntityId, EntityId>,
}

#[derive(Component)]
pub struct GuiPropProxyEntity {
    #[allow(dead_code)]
    entity_id: EntityId,
}

impl GuiManager {
    pub fn new() -> GuiManager {
        GuiManager {
            handle_to_instance: HashMap::new(),
            entity_id_to_proxy_entity_id: HashMap::new(),
        }
    }

    pub fn update_ui(
        &mut self,
        world: &mut World,
        physics: &mut PhysicsWorld,
        scripts: &mut ScriptWorld,
        id_to_physics: &mut HashMap<EntityId, RigidBodyHandle>,
        handle: GuiHandle,
        parent_entity: EntityId,
        world_size: Vector2<f32>,
        offset: Vector3<f32>,
        components: Vec<GuiComponentRenderInfo>,
    ) {
        if let std::collections::hash_map::Entry::Vacant(e) = self.handle_to_instance.entry(handle)
        {
            // Add proxy entity to world
            let ent = world.add_entity((
                GuiPropProxyEntity {
                    entity_id: parent_entity,
                },
                RuntimePropDoNotSerialize,
            ));
            self.entity_id_to_proxy_entity_id.insert(parent_entity, ent);

            log_entity(world, parent_entity);
            let pos = get_position_from_transform(world, parent_entity, offset);
            let facing = get_rotation_from_transform(world, parent_entity);
            // Create physics for this entity
            let physics_handle = physics.add_kinematic(
                ent,
                pos.to_vec(),
                facing,
                vec3(0.0, 0.0, 0.0),
                vec3(world_size.x, world_size.y, 0.0),
                CollisionGroup::ui(),
                false,
            );
            id_to_physics.insert(ent, physics_handle);

            let script = Box::new(ProxyGuiScript::new(world_size, parent_entity));
            scripts.add_entity2(ent, script);

            e.insert(GuiInstanceInfo {
                parent_entity,
                proxy_entity: ent,
                offset,
                components,
                world_size,
                physics_handle,
            });
        } else {
            let instance: &mut GuiInstanceInfo = self.handle_to_instance.get_mut(&handle).unwrap();
            instance.components = components;
            let pos = get_position_from_transform(world, parent_entity, offset);
            let facing = get_rotation_from_transform(world, parent_entity);
            physics.set_position_rotation(instance.physics_handle, pos.to_vec(), facing)
        }
    }

    pub fn update(&mut self) {}

    pub fn render(&mut self, asset_cache: &mut AssetCache, world: &World) -> Vec<SceneObject> {
        let mut ret = Vec::new();
        let v_transform = world.borrow::<View<RuntimePropTransform>>().unwrap();
        for (_handle, info) in &self.handle_to_instance {
            let player_mat = engine::scene::color_material::create(Vector3::new(0.0, 0.0, 1.0));
            let mut gui_obj = SceneObject::new(player_mat, Box::new(engine::scene::quad::create()));
            let maybe_transform = v_transform.get(info.proxy_entity);

            if maybe_transform.is_err() {
                continue;
            }

            let parent_entity_transform = maybe_transform.unwrap().0;
            let _root_transform = parent_entity_transform;
            // * Matrix4::from_translation(info.offset)
            // * Matrix4::from_nonuniform_scale(0.5, 0.6, 1.0);
            let root_transform = parent_entity_transform
            //     * Matrix4::from_translation(info.offset)
                * Matrix4::from_nonuniform_scale(info.world_size.x,info.world_size.y, 1.0);
            gui_obj.set_transform(root_transform);

            for component in &info.components {
                let mut comp_obj = component.render(asset_cache);
                comp_obj.set_transform(root_transform);
                ret.push(comp_obj);
            }

            // gui_obj.set_local_transform(
            //     Matrix4::from_translation(info.offset)
            //         * Matrix4::from_nonuniform_scale(0.5, 0.6, 1.0),
            // );
            //ret.push(gui_obj);
        }
        ret
    }
}
