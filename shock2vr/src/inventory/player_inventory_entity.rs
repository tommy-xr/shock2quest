use cgmath::{vec3, Matrix4, Quaternion, Vector3};
use dark::properties::{Links, PropPosition, PropScripts, PropTemplateId};
use engine::scene::SceneObject;
use shipyard::{Component, EntityId, IntoIter, View, ViewMut, World};

use crate::runtime_props::RuntimePropTransform;

/// Invalid/null template ID used when no valid template is assigned
const INVALID_TEMPLATE_ID: i32 = -1;

#[derive(Component, Clone, Debug, PartialEq)]
pub struct PlayerInventoryEntity {}

impl PlayerInventoryEntity {
    pub fn create(world: &mut World) -> EntityId {
        world.add_entity((
            PlayerInventoryEntity {},
            Links::empty(),
            PropScripts {
                scripts: vec!["internal_inventory".to_owned()],
                inherits: true,
            },
            PropTemplateId {
                template_id: INVALID_TEMPLATE_ID,
            },
            PropPosition {
                position: Vector3::new(0.0, 1.0, 0.0),
                rotation: cgmath::Quaternion {
                    v: vec3(0.0, 0.0, 0.0),
                    s: 1.0,
                },
                cell: 0,
            },
            RuntimePropTransform(Matrix4::from_translation(vec3(0.0, 1.0, 0.0))),
        ))
    }

    pub fn render(world: &World) -> Vec<SceneObject> {
        let mut ret = Vec::with_capacity(1);
        let (player_inventory_entities, position) = world
            .borrow::<(View<PlayerInventoryEntity>, View<PropPosition>)>()
            .unwrap();
        for (_player_inventory_entity, position) in (&player_inventory_entities, &position).iter() {
            let inventory_material =
                engine::scene::color_material::create(Vector3::new(0.0, 0.0, 1.0));
            let mut cube =
                SceneObject::new(inventory_material, Box::new(engine::scene::cube::create()));

            let transform = Matrix4::from_translation(position.position)
                * Matrix4::from(position.rotation)
                * Matrix4::from_scale(0.1);
            cube.set_transform(transform);

            ret.push(cube);
        }

        ret
    }

    pub fn set_position_rotation(
        world: &mut World,
        position: Vector3<f32>,
        rotation: Quaternion<f32>,
    ) {
        let transform = Matrix4::from_translation(position) * Matrix4::from(rotation);
        let player_inventory_entities = world.borrow::<View<PlayerInventoryEntity>>().unwrap();
        let mut prop_position = world.borrow::<ViewMut<PropPosition>>().unwrap();
        let mut prop_transform = world.borrow::<ViewMut<RuntimePropTransform>>().unwrap();
        for (_player_inventory_entity, p, xform) in (
            &player_inventory_entities,
            &mut prop_position,
            &mut prop_transform,
        )
            .iter()
        {
            p.position = position;
            p.rotation = rotation;

            xform.0 = transform;
        }
    }
}
