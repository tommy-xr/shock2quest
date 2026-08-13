use std::collections::HashMap;

use cgmath::{Deg, EuclideanSpace, InnerSpace, Matrix4, Vector2, Vector3, vec3};

use engine::{assets::asset_cache::AssetCache, scene::SceneObject};
use rapier3d::prelude::RigidBodyHandle;
use shipyard::{Component, EntitiesView, EntityId, Get, UniqueView, View, World};

use crate::{
    mission::PlayerInfo,
    physics::{CollisionGroup, PhysicsWorld},
    runtime_props::{RuntimePropDoNotSerialize, RuntimePropTransform},
    scripts::ScriptWorld,
    util::{get_position_from_transform, get_rotation_from_transform, log_entity},
};

use crate::gui::*;
use crate::ui::{Rect, UiCanvas};

/// Depth given to each successive panel element so a label does not z-fight
/// with the art behind it. Canvas-local +Z faces the viewer (the root
/// transform below turns the proxy entity's -Z face into that convention),
/// which is the direction `UiCanvas::render_world_space` steps in.
const COMPONENT_Z_STEP: f32 = 0.001;

/// Total depth a panel may spend on that separation. MFD panels are small
/// (a keypad is ~0.75 m) and carry dozens of components - a full inventory is
/// 15x3 slots - so an unbounded per-element step would push the last elements
/// centimetres off the panel plane and visibly parallax in VR. The step
/// shrinks to fit instead.
const PANEL_DEPTH_BUDGET: f32 = 0.01;

pub struct GuiInstanceInfo {
    pub parent_entity: EntityId,
    pub proxy_entity: EntityId,
    #[allow(dead_code)]
    pub offset: Vector3<f32>,
    pub components: Vec<GuiComponentRenderInfo>,
    /// Authored layout extent. This stays in canvas pixels even when a VR
    /// presentation maps the panel onto a differently-sized physical quad.
    pub canvas_size_px: Vector2<f32>,
    pub world_size: Vector2<f32>,
    pub physics_handle: RigidBodyHandle,
}

pub struct GuiManager {
    handle_to_instance: HashMap<GuiHandle, GuiInstanceInfo>,
    entity_id_to_proxy_entity_id: HashMap<EntityId, EntityId>,
    /// The single object-bound world panel opened through `Effect::OpenPanel`
    /// in default VR. The old `--experimental gui` mode still materializes
    /// every panel and does not consult this slot.
    active_panel: Option<EntityId>,
}

/// Match the original container overlay's distance-close behavior. Four world
/// units is beyond normal frob reach, so small looting movements keep the
/// panel open while walking away removes its transient collider and art.
const PANEL_AUTO_CLOSE_DISTANCE: f32 = 4.0;

#[derive(Component)]
pub struct GuiPropProxyEntity {
    #[allow(dead_code)]
    entity_id: EntityId,
}

#[derive(Component, Clone, Copy)]
pub struct GuiPropProxySize(pub Vector2<f32>);

impl GuiManager {
    pub fn new() -> GuiManager {
        GuiManager {
            handle_to_instance: HashMap::new(),
            entity_id_to_proxy_entity_id: HashMap::new(),
            active_panel: None,
        }
    }

    pub fn active_panel(&self) -> Option<EntityId> {
        self.active_panel
    }

    /// Toggle one gameplay entity in the default-VR world-panel slot.
    ///
    /// Player-owned panels such as the backpack have no world object to frob,
    /// so their production controller binding reaches the same panel lifecycle
    /// directly. Object-bound panels still enter through `Effect::OpenPanel`.
    pub fn toggle_panel(
        &mut self,
        entity: EntityId,
        world: &mut World,
        physics: &mut PhysicsWorld,
        scripts: &mut ScriptWorld,
        id_to_physics: &mut HashMap<EntityId, RigidBodyHandle>,
    ) {
        if self.active_panel == Some(entity) {
            self.close_panel(world, physics, scripts, id_to_physics);
        } else {
            self.open_panel(entity, false, world, physics, scripts, id_to_physics);
        }
    }

    /// Bind the default-VR world-panel slot to one gameplay entity. Opening a
    /// second object removes the first panel's transient proxy, just as the
    /// original left MFD slot replaces the previous object-bound overlay.
    ///
    /// `retain_existing` preserves the legacy `--experimental gui` behavior,
    /// where all authored panels are materialized at once.
    pub fn open_panel(
        &mut self,
        entity: EntityId,
        retain_existing: bool,
        world: &mut World,
        physics: &mut PhysicsWorld,
        scripts: &mut ScriptWorld,
        id_to_physics: &mut HashMap<EntityId, RigidBodyHandle>,
    ) {
        if self.active_panel != Some(entity) && !retain_existing {
            if let Some(previous) = self.active_panel {
                self.remove_parent_instances(previous, world, physics, scripts, id_to_physics);
            }
        }
        self.active_panel = Some(entity);
    }

    /// Close the active default-VR panel and remove its non-serialized proxy.
    pub fn close_panel(
        &mut self,
        world: &mut World,
        physics: &mut PhysicsWorld,
        scripts: &mut ScriptWorld,
        id_to_physics: &mut HashMap<EntityId, RigidBodyHandle>,
    ) {
        if let Some(parent) = self.active_panel.take() {
            self.remove_parent_instances(parent, world, physics, scripts, id_to_physics);
        }
    }

    /// Preserve the original overlay's close conditions in world space: a
    /// destroyed host or a player walking away dismisses the panel. Carried
    /// panel hosts are exempt because their authored world position is stale.
    pub fn maintain_active_panel(
        &mut self,
        world: &mut World,
        physics: &mut PhysicsWorld,
        scripts: &mut ScriptWorld,
        id_to_physics: &mut HashMap<EntityId, RigidBodyHandle>,
    ) {
        let Some(parent) = self.active_panel else {
            return;
        };
        let alive = world
            .borrow::<EntitiesView>()
            .map(|entities| entities.is_alive(parent))
            .unwrap_or(false);
        let carried =
            alive && crate::scripts::script_util::player_carried_items(world).contains(&parent);
        let too_far = alive
            && !carried
            && world
                .borrow::<UniqueView<PlayerInfo>>()
                .map(|player| {
                    let position = get_position_from_transform(world, parent, vec3(0.0, 0.0, 0.0));
                    (position.to_vec() - player.pos).magnitude() > PANEL_AUTO_CLOSE_DISTANCE
                })
                .unwrap_or(false);
        if !alive || too_far {
            self.close_panel(world, physics, scripts, id_to_physics);
        }
    }

    /// Remove GUI state owned by a gameplay entity before that entity is
    /// deleted. This also handles a proxy being removed independently, so no
    /// recycled `EntityId` can inherit stale panel state.
    pub fn on_entity_destroyed(
        &mut self,
        entity: EntityId,
        world: &mut World,
        physics: &mut PhysicsWorld,
        scripts: &mut ScriptWorld,
        id_to_physics: &mut HashMap<EntityId, RigidBodyHandle>,
    ) {
        if self.active_panel == Some(entity) {
            self.active_panel = None;
        }
        self.remove_parent_instances(entity, world, physics, scripts, id_to_physics);

        let handles: Vec<_> = self
            .handle_to_instance
            .iter()
            .filter_map(|(handle, info)| (info.proxy_entity == entity).then_some(*handle))
            .collect();
        for handle in handles {
            self.handle_to_instance.remove(&handle);
        }
        self.entity_id_to_proxy_entity_id
            .retain(|_, proxy| *proxy != entity);
    }

    fn remove_parent_instances(
        &mut self,
        parent: EntityId,
        world: &mut World,
        physics: &mut PhysicsWorld,
        scripts: &mut ScriptWorld,
        id_to_physics: &mut HashMap<EntityId, RigidBodyHandle>,
    ) {
        let handles: Vec<_> = self
            .handle_to_instance
            .iter()
            .filter_map(|(handle, info)| (info.parent_entity == parent).then_some(*handle))
            .collect();
        for handle in handles {
            if let Some(instance) = self.handle_to_instance.remove(&handle) {
                let proxy = instance.proxy_entity;
                self.entity_id_to_proxy_entity_id.remove(&parent);
                id_to_physics.remove(&proxy);
                physics.remove(proxy);
                scripts.remove_entity(proxy);
                let alive = world
                    .borrow::<EntitiesView>()
                    .map(|entities| entities.is_alive(proxy))
                    .unwrap_or(false);
                if alive {
                    world.delete_entity(proxy);
                }
            }
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
        canvas_size_px: Vector2<f32>,
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
                GuiPropProxySize(world_size),
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

            let script = Box::new(ProxyGuiScript::new(parent_entity));
            scripts.add_entity2(ent, script);

            e.insert(GuiInstanceInfo {
                parent_entity,
                proxy_entity: ent,
                offset,
                components,
                canvas_size_px,
                world_size,
                physics_handle,
            });
        } else {
            let instance: &mut GuiInstanceInfo = self.handle_to_instance.get_mut(&handle).unwrap();
            instance.components = components;
            instance.canvas_size_px = canvas_size_px;
            if instance.world_size != world_size {
                instance.world_size = world_size;
                world.add_component(instance.proxy_entity, GuiPropProxySize(world_size));
                physics.resize_kinematic_cuboid(
                    instance.physics_handle,
                    instance.proxy_entity,
                    vec3(world_size.x, world_size.y, 0.0),
                );
            }
            let pos = get_position_from_transform(world, parent_entity, offset);
            let facing = get_rotation_from_transform(world, parent_entity);
            physics.set_position_rotation(instance.physics_handle, pos.to_vec(), facing)
        }
    }

    pub fn update(&mut self) {}

    pub fn render(&mut self, asset_cache: &mut AssetCache, world: &World) -> Vec<SceneObject> {
        self.render_filtered(asset_cache, world, false)
    }

    /// Render only the object-bound default-VR slot. Experimental GUI keeps
    /// using [`render`](Self::render) to show every authored panel.
    pub fn render_active(
        &mut self,
        asset_cache: &mut AssetCache,
        world: &World,
    ) -> Vec<SceneObject> {
        self.render_filtered(asset_cache, world, true)
    }

    fn render_filtered(
        &mut self,
        asset_cache: &mut AssetCache,
        world: &World,
        active_only: bool,
    ) -> Vec<SceneObject> {
        let mut ret = Vec::new();
        let v_transform = world.borrow::<View<RuntimePropTransform>>().unwrap();
        for (_handle, info) in &self.handle_to_instance {
            if active_only && Some(info.parent_entity) != self.active_panel {
                continue;
            }
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
            // The proxy entity's outward face is its local -Z, while the
            // world-space canvas convention is "+Z faces the viewer" - so turn
            // the panel around here, at the one boundary where the entity's
            // authored facing meets the shared canvas path.
            let root_transform = parent_entity_transform
                * Matrix4::from_angle_y(Deg(180.0))
                * Matrix4::from_nonuniform_scale(info.world_size.x, info.world_size.y, 1.0);
            gui_obj.set_transform(root_transform);

            // Present the panel through the shared UI canvas, so the world
            // panel and the flat MFD overlay are two mappings of ONE layout
            // rather than two hand-written emit paths.
            let size_px = info.canvas_size_px;
            let panel = Rect::new(0.0, 0.0, size_px.x, size_px.y);
            let elements = info
                .components
                .iter()
                .map(|component| component.to_ui_element(panel))
                .collect();
            let canvas = UiCanvas::from_elements(size_px, elements);
            let z_step =
                COMPONENT_Z_STEP.min(PANEL_DEPTH_BUDGET / canvas.element_count().max(1) as f32);
            ret.extend(canvas.render_world_space(asset_cache, root_transform, None, None, z_step));

            // gui_obj.set_local_transform(
            //     Matrix4::from_translation(info.offset)
            //         * Matrix4::from_nonuniform_scale(0.5, 0.6, 1.0),
            // );
            //ret.push(gui_obj);
        }
        ret
    }
}

#[cfg(test)]
mod tests {
    use cgmath::{Matrix4, Quaternion, vec2, vec3};
    use shipyard::{EntitiesView, Get};

    use super::*;

    #[test]
    fn live_panel_resize_updates_render_input_and_collider_geometry() {
        let mut world = World::new();
        let parent = world.add_entity((RuntimePropTransform(Matrix4::from_scale(1.0)),));
        let mut physics = PhysicsWorld::new();
        let mut scripts = ScriptWorld::new();
        let mut id_to_physics = HashMap::new();
        let mut manager = GuiManager::new();
        let handle = GuiHandle::new();

        manager.update_ui(
            &mut world,
            &mut physics,
            &mut scripts,
            &mut id_to_physics,
            handle,
            parent,
            vec2(261.0, 296.0),
            vec2(261.0, 296.0),
            vec3(0.0, 0.0, -1.0),
            Vec::new(),
        );
        manager.update_ui(
            &mut world,
            &mut physics,
            &mut scripts,
            &mut id_to_physics,
            handle,
            parent,
            vec2(188.0, 296.0),
            vec2(188.0, 296.0),
            vec3(0.0, 0.0, -1.0),
            Vec::new(),
        );

        let instance = manager.handle_to_instance.get(&handle).unwrap();
        assert_eq!(instance.canvas_size_px, vec2(188.0, 296.0));
        assert_eq!(instance.world_size, vec2(188.0, 296.0));
        assert_eq!(
            world
                .borrow::<View<GuiPropProxySize>>()
                .unwrap()
                .get(instance.proxy_entity)
                .unwrap()
                .0,
            vec2(188.0, 296.0)
        );
        assert_eq!(
            physics.cuboid_full_size(instance.physics_handle),
            Some(vec3(188.0, 296.0, 0.01))
        );
    }

    #[test]
    fn opening_a_second_default_panel_removes_the_first_transient_proxy() {
        let mut world = World::new();
        let first = world.add_entity((RuntimePropTransform(Matrix4::from_scale(1.0)),));
        let second = world.add_entity((RuntimePropTransform(Matrix4::from_scale(1.0)),));
        let mut physics = PhysicsWorld::new();
        let mut scripts = ScriptWorld::new();
        let mut id_to_physics = HashMap::new();
        let mut manager = GuiManager::new();

        manager.open_panel(
            first,
            false,
            &mut world,
            &mut physics,
            &mut scripts,
            &mut id_to_physics,
        );
        manager.update_ui(
            &mut world,
            &mut physics,
            &mut scripts,
            &mut id_to_physics,
            GuiHandle::new(),
            first,
            vec2(187.5, 295.0),
            vec2(0.75, 1.18),
            vec3(0.0, 1.0, 0.0),
            Vec::new(),
        );
        let first_instance = manager.handle_to_instance.values().next().unwrap();
        let first_proxy = first_instance.proxy_entity;
        let first_physics = first_instance.physics_handle;

        manager.open_panel(
            second,
            false,
            &mut world,
            &mut physics,
            &mut scripts,
            &mut id_to_physics,
        );

        assert_eq!(manager.active_panel(), Some(second));
        assert!(manager.handle_to_instance.is_empty());
        assert!(!id_to_physics.contains_key(&first_proxy));
        assert!(physics.get_position(first_physics).is_none());
        assert!(
            !world
                .borrow::<EntitiesView>()
                .unwrap()
                .is_alive(first_proxy)
        );
    }

    #[test]
    fn toggling_a_player_panel_closes_its_transient_proxy() {
        let mut world = World::new();
        let parent = world.add_entity((RuntimePropTransform(Matrix4::from_scale(1.0)),));
        let mut physics = PhysicsWorld::new();
        let mut scripts = ScriptWorld::new();
        let mut id_to_physics = HashMap::new();
        let mut manager = GuiManager::new();

        manager.toggle_panel(
            parent,
            &mut world,
            &mut physics,
            &mut scripts,
            &mut id_to_physics,
        );
        assert_eq!(manager.active_panel(), Some(parent));

        manager.update_ui(
            &mut world,
            &mut physics,
            &mut scripts,
            &mut id_to_physics,
            GuiHandle::new(),
            parent,
            vec2(635.0, 120.0),
            vec2(2.54, 0.48),
            vec3(0.0, 1.0, 0.0),
            Vec::new(),
        );
        assert_eq!(manager.handle_to_instance.len(), 1);
        assert_eq!(id_to_physics.len(), 1);

        manager.toggle_panel(
            parent,
            &mut world,
            &mut physics,
            &mut scripts,
            &mut id_to_physics,
        );
        assert_eq!(manager.active_panel(), None);
        assert!(manager.handle_to_instance.is_empty());
        assert!(id_to_physics.is_empty());
    }

    #[test]
    fn walking_beyond_the_overlay_distance_closes_the_default_panel() {
        let mut world = World::new();
        let inventory = world.add_entity(());
        let player_entity = world.add_entity(());
        let parent = world.add_entity((RuntimePropTransform(Matrix4::from_translation(vec3(
            PANEL_AUTO_CLOSE_DISTANCE + 1.0,
            0.0,
            0.0,
        ))),));
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player_entity,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory,
        });
        let mut physics = PhysicsWorld::new();
        let mut scripts = ScriptWorld::new();
        let mut id_to_physics = HashMap::new();
        let mut manager = GuiManager::new();
        manager.open_panel(
            parent,
            false,
            &mut world,
            &mut physics,
            &mut scripts,
            &mut id_to_physics,
        );
        manager.update_ui(
            &mut world,
            &mut physics,
            &mut scripts,
            &mut id_to_physics,
            GuiHandle::new(),
            parent,
            vec2(187.5, 295.0),
            vec2(0.75, 1.18),
            vec3(0.0, 1.0, 0.0),
            Vec::new(),
        );

        manager.maintain_active_panel(&mut world, &mut physics, &mut scripts, &mut id_to_physics);

        assert_eq!(manager.active_panel(), None);
        assert!(manager.handle_to_instance.is_empty());
        assert!(id_to_physics.is_empty());
    }
}
