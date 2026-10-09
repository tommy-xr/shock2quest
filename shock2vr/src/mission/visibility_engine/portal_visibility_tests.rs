use super::*;
use cgmath::Quaternion;
use shipyard::ViewMut;

fn cell(position: Vector3<f32>) -> Option<u32> {
    if position.z > 10.0 {
        None
    } else {
        Some(u32::from(position.x >= 0.0))
    }
}

fn membership(position: Vector3<f32>, size: Option<Vector3<f32>>, cells: &mut Vec<u32>) {
    // Fake BSP is the x=0 split, plus an empty region above z=10.
    let half = size.unwrap_or(vec3(0.0, 0.0, 0.0)) * 0.5;
    if position.z - half.z > 10.0 {
        return;
    }
    if position.x - half.x < 0.0 {
        cells.push(0);
    }
    if position.x + half.x >= 0.0 {
        cells.push(1);
    }
}

// Independent reference with the original corner ordering and short circuit.
fn reference(position: Vector3<f32>, size: Option<Vector3<f32>>, visible: &HashSet<u32>) -> bool {
    let points = if let Some(size) = size {
        let h = size * 0.5;
        vec![
            position + vec3(-h.x, -h.y, -h.z),
            position + vec3(h.x, -h.y, -h.z),
            position + vec3(-h.x, h.y, -h.z),
            position + vec3(h.x, h.y, -h.z),
            position + vec3(-h.x, -h.y, h.z),
            position + vec3(h.x, -h.y, h.z),
            position + vec3(-h.x, h.y, h.z),
            position + vec3(h.x, h.y, h.z),
        ]
    } else {
        vec![position]
    };
    points
        .into_iter()
        .any(|p| cell(p).is_some_and(|c| visible.contains(&c)))
}

#[test]
fn membership_matches_reference_across_camera_motion_teleports_and_resizing() {
    let mut cached = EntityCells::new(vec3(0.0, 0.0, 0.0), None);
    for position in [
        vec3(-4.0, 0.0, 0.0),
        vec3(0.0, 0.0, 0.0),
        vec3(4.0, 0.0, 0.0),
        vec3(0.0, 0.0, 11.0),
    ] {
        for size in [
            None,
            Some(vec3(2.0, 2.0, 2.0)),
            Some(vec3(12.0, 4.0, 4.0)),
            None,
        ] {
            for visible in [
                HashSet::from([0]),
                HashSet::from([1]),
                HashSet::new(),
                HashSet::from([0, 1]),
                HashSet::from([0]),
            ] {
                assert_eq!(
                    cached.is_visible(position, size, &visible, &mut membership),
                    reference(position, size, &visible)
                );
            }
        }
    }
}

#[test]
fn caches_empty_coverage_and_refreshes_moving_objects_once() {
    let mut cache = EntityCells::new(vec3(0.0, 0.0, 0.0), None);
    let size = Some(vec3(2.0, 2.0, 2.0));
    let mut calls = 0;
    let mut lookup = |p, size, cells: &mut Vec<u32>| {
        calls += 1;
        membership(p, size, cells);
    };
    for x in [-4.0, -3.0, -2.0] {
        assert!(cache.is_visible(vec3(x, 0.0, 0.0), size, &HashSet::from([0]), &mut lookup));
    }
    assert_eq!(calls, 3);
    let mut calls = 0;
    for _ in 0..10 {
        assert!(!cache.is_visible(
            vec3(0.0, 0.0, 20.0),
            size,
            &HashSet::from([0, 1]),
            &mut |p, size, cells| {
                calls += 1;
                membership(p, size, cells);
            }
        ));
    }
    assert_eq!(calls, 1, "empty BSP coverage is cached too");
}

fn position(x: f32) -> PropPosition {
    PropPosition {
        position: vec3(x, 0.0, 0.0),
        cell: 0,
        rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
    }
}

#[test]
fn entity_lifecycle_and_has_refs_are_not_cached() {
    let mut world = World::new();
    let id = world.add_entity((position(-1.0), PropHasRefs(true)));
    let mut engine = PortalVisibilityEngine::new();
    let visible = HashSet::from([0]);
    engine.update_entities(&world, &visible, membership);
    assert!(engine.is_visible(id));
    (&mut world.borrow::<ViewMut<PropHasRefs>>().unwrap())
        .get(id)
        .unwrap()
        .0 = false;
    engine.update_entities(&world, &visible, membership);
    assert!(!engine.is_visible(id));
    (&mut world.borrow::<ViewMut<PropHasRefs>>().unwrap())
        .get(id)
        .unwrap()
        .0 = true;
    engine.update_entities(&world, &visible, membership);
    assert!(engine.is_visible(id));
    world.remove::<(PropPosition,)>(id);
    engine.update_entities(&world, &visible, membership);
    assert!(!engine.is_visible(id));
    assert!(engine.entity_cell_cache.is_empty());
    world.add_component(id, (position(1.0),));
    engine.update_entities(&world, &visible, membership);
    assert!(!engine.is_visible(id));
    world.delete_entity(id);
    engine.update_entities(&world, &visible, membership);
    assert!(engine.entity_cell_cache.is_empty());
    assert!(!engine.is_visible(id));
}
