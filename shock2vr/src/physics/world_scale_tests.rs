//! One process owns the global scale; library tests keep their default scale.
use crate::{
    dev_params,
    physics::{CollisionGroup, PhysicsWorld},
    world_scale::WorldScale,
};
use cgmath::{InnerSpace, vec3};
use rapier3d::prelude::{Isometry, SharedShape};
use shipyard::EntityId;

#[test]
fn live_scale_preserves_floor_tracking_and_native_geometry_and_defers_blocked_growth() {
    // Isolate global developer state from every other library test.
    if std::env::var_os("SHOCK2_WORLD_SCALE_TEST_CHILD").is_none() {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "physics::world_scale_tests::live_scale_preserves_floor_tracking_and_native_geometry_and_defers_blocked_growth", "--nocapture"])
            .env("SHOCK2_WORLD_SCALE_TEST_CHILD", "1").status().unwrap();
        assert!(status.success());
        return;
    }
    let mut world = PhysicsWorld::new();
    let floor = world.create_static_body(
        Isometry::translation(0.0, -1.0, 0.0),
        EntityId::from_inner(1000),
    );
    world.attach_collider(
        floor,
        SharedShape::cuboid(20.0, 1.0, 20.0),
        1.0,
        CollisionGroup::entity(),
    );
    // A 2 m ceiling fits the baseline player but not the expanded 0.82x capsule.
    let roof = world.create_static_body(
        Isometry::translation(0.0, 2.0 / crate::METERS_PER_WORLD_UNIT + 0.1, 0.0),
        EntityId::from_inner(1002),
    );
    world.attach_collider(
        roof,
        SharedShape::cuboid(2.0, 0.1, 2.0),
        1.0,
        CollisionGroup::entity(),
    );
    let mut player = world.create_player(vec3(0.0, 1.3, 0.0), EntityId::from_inner(1001).unwrap());
    for _ in 0..120 {
        world.update(vec3(0.0, 0.0, 0.0), &mut player);
    }
    assert!(player.is_grounded());
    dev_params::set(dev_params::WORLD_SCALE, 0.82);
    assert!(
        !world.update_world_scale(&mut player),
        "ceiling must defer expansion"
    );
    assert_eq!(WorldScale::active().factor(), 1.0);

    world.set_player_translation(vec3(5.0, 1.3, 0.0), &mut player);
    for _ in 0..120 {
        world.update(vec3(0.0, 0.0, 0.0), &mut player);
    }
    let before = world.get_player_translation(&player);
    let native = world
        .debug_list_bodies()
        .into_iter()
        .filter(|b| b.body_type == "static")
        .map(|b| (b.body_id, b.position))
        .collect::<Vec<_>>();
    assert_eq!(native.len(), 2);
    let head = vec3(0.0, 1.74, 0.0);
    let hand = head + vec3(0.3, -0.25, -0.4);
    let mut input = crate::input_context::InputContext::default();
    let tracking = crate::vr_tracking::TrackingTransform::new(
        crate::physics::player_center_above_floor(false),
        crate::physics::player_eye_cap_above_center(false),
        head.y,
        0.0,
    );
    input.tracking = Some(tracking);
    input.head.position = tracking.stage_to_pawn(head);
    input.right_hand.position = tracking.stage_to_pawn(hand);

    let library: crate::vr_grip::GripLibrary =
        serde_json::from_str(include_str!("../../../assets/vr-tricorder-grips.json")).unwrap();
    let panels = |unit: f32| {
        ["left", "right"].map(|hand| {
            let grip = library
                .lookup("tricorder", hand)
                .unwrap()
                .clone()
                .for_world_scale();
            let panel = crate::mission::mfd_device::panel(
                vec3(0.0, 0.0, 0.0),
                cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0),
                Some(&grip),
                usize::from(hand == "right"),
            )
            .unwrap();
            (panel.size * unit, panel.center * unit)
        })
    };
    let baseline_panels = panels(crate::METERS_PER_WORLD_UNIT);

    for factor in [0.82, 1.0, 0.82, 1.0] {
        let canonical_before =
            world.get_player_translation(&player).y - crate::physics::player_scale_center_offset();
        dev_params::set(dev_params::WORLD_SCALE, factor);
        assert!(world.update_world_scale(&mut player));
        assert!(!world.update_world_scale(&mut player), "apply once");
        let center = crate::physics::player_center_above_floor(false);
        let cap = crate::physics::player_eye_cap_above_center(false);
        crate::vr_tracking::TrackingTransform::rebase_input(&mut input, center, cap);
        let unit = crate::world_scale::meters_per_world_unit();
        for (current, baseline) in panels(unit).iter().zip(&baseline_panels) {
            assert!(
                (current.0 - baseline.0).magnitude() < 1e-5,
                "MFD physical size"
            );
            assert!(
                (current.1 - baseline.1).magnitude() < 1e-5,
                "MFD grip placement"
            );
        }
        assert!(((input.head.position.y + center) * unit - head.y).abs() < 1e-5);
        assert!(
            ((input.right_hand.position - input.head.position) * unit - (hand - head)).magnitude()
                < 1e-5
        );
        let transform = input.tracking.unwrap();
        let ipd = transform.stage_to_pawn(head + vec3(0.032, 0.0, 0.0))
            - transform.stage_to_pawn(head - vec3(0.032, 0.0, 0.0));
        assert!((ipd.magnitude() * unit - 0.064).abs() < 1e-5);
        let position = world.get_player_translation(&player);
        assert!(
            (position.y - crate::physics::player_scale_center_offset() - canonical_before).abs()
                < 1e-5,
            "save center and floor must stay fixed"
        );
        for _ in 0..60 {
            world.update(vec3(0.0, 0.0, 0.0), &mut player);
        }
        assert!(player.is_grounded());
        assert!(
            (world.get_player_translation(&player).y - position.y).abs() < 0.001,
            "the resized collider must settle on the same floor anchor"
        );
        assert!(
            (world.get_player_translation(&player).y
                - crate::physics::player_scale_center_offset()
                - before.y)
                .abs()
                < 0.001,
            "contact tolerance cannot accumulate floor drift"
        );
        assert_eq!(
            native,
            world
                .debug_list_bodies()
                .into_iter()
                .filter(|b| b.body_type == "static")
                .map(|b| (b.body_id, b.position))
                .collect::<Vec<_>>()
        );
    }
    dev_params::set(dev_params::WORLD_SCALE, 0.82);
    assert!(world.update_world_scale(&mut player));
    super::tests::player_tops_out_from_a_ladder_onto_an_upper_landing();
}
