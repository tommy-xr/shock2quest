//! Shared plumbing for player-owned panel hosts. Panel-specific subjects and
//! presentation policies stay with their callers in `MissionCore`.

use cgmath::{Matrix4, Quaternion, SquareMatrix, vec3};
use dark::properties::{Links, PropPosition, PropScripts, PropSymName, PropTemplateId};
use shipyard::{EntityId, World};

use super::flat_ui_host::FlatUiHost;
use crate::runtime_props::{RuntimePropDoNotSerialize, RuntimePropTransform};

/// Create an unbound host with no authored object. Rebuilt each mission/load;
/// callers attach panel-specific data and register the corresponding unique.
pub(super) fn spawn(world: &mut World, script: &str, name: Option<&str>) -> EntityId {
    let entity = world.add_entity((
        Links::empty(),
        PropScripts {
            scripts: vec![script.to_owned()],
            inherits: false,
        },
        PropTemplateId { template_id: -1 },
        PropPosition {
            position: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            cell: 0,
        },
        RuntimePropTransform(Matrix4::identity()),
        RuntimePropDoNotSerialize,
    ));
    if let Some(name) = name {
        world.add_component(entity, PropSymName(name.to_owned()));
    }
    entity
}

/// Return whether the caller should retain its active subject/flag. Dismiss
/// only our own panel: a replacement panel must survive stale subject cleanup.
/// The caller decides when to dismiss (including device/result-overlay rules).
pub(super) fn retain_docked(host: &mut FlatUiHost, panel: Option<EntityId>, dismiss: bool) -> bool {
    if panel.is_none() || host.active_panel() != panel {
        return false;
    }
    if dismiss {
        host.close();
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use shipyard::{Get, View};

    #[test]
    fn hosts_have_identity_transforms_scripts_and_no_serialized_world_object() {
        let mut world = World::new();
        let named = spawn(&mut world, "internal_media", Some("Audio Log Reader"));
        let unnamed = spawn(&mut world, "internal_map", None);
        let (positions, transforms, scripts, templates, transient, names, links) = world
            .borrow::<(
                View<PropPosition>,
                View<RuntimePropTransform>,
                View<PropScripts>,
                View<PropTemplateId>,
                View<RuntimePropDoNotSerialize>,
                View<PropSymName>,
                View<Links>,
            )>()
            .unwrap();
        for entity in [named, unnamed] {
            let position = (&positions).get(entity).unwrap();
            assert_eq!(position.position, vec3(0.0, 0.0, 0.0));
            assert_eq!(position.rotation, Quaternion::new(1.0, 0.0, 0.0, 0.0));
            assert_eq!(position.cell, 0);
            assert_eq!((&transforms).get(entity).unwrap().0, Matrix4::identity());
            assert_eq!((&templates).get(entity).unwrap().template_id, -1);
            assert!((&transient).get(entity).is_ok());
            assert!((&links).get(entity).is_ok());
            assert!(!(&scripts).get(entity).unwrap().inherits);
        }
        assert_eq!((&scripts).get(named).unwrap().scripts, ["internal_media"]);
        assert_eq!((&scripts).get(unnamed).unwrap().scripts, ["internal_map"]);
        assert_eq!((&names).get(named).unwrap().0, "Audio Log Reader");
        assert!((&names).get(unnamed).is_err());
    }

    #[test]
    fn docked_subject_lifetime_never_closes_a_replacement_panel() {
        let mut world = World::new();
        let panel = world.add_entity(());
        let replacement = world.add_entity(());
        let mut host = FlatUiHost::new();

        host.open_unbound(panel);
        assert!(retain_docked(&mut host, Some(panel), false));
        assert_eq!(host.active_panel(), Some(panel));
        assert!(!retain_docked(&mut host, Some(panel), true));
        assert_eq!(host.active_panel(), None);

        host.open_unbound(replacement);
        for dismiss in [false, true] {
            assert!(!retain_docked(&mut host, Some(panel), dismiss));
            assert!(!retain_docked(&mut host, None, dismiss));
            assert_eq!(host.active_panel(), Some(replacement));
        }
        host.close();
        assert!(!retain_docked(&mut host, Some(panel), false));
    }
}
