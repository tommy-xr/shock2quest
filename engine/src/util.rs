use cgmath::{Matrix4, Quaternion, SquareMatrix, Vector2, Vector3, Vector4};

use crate::EngineRenderContext;

pub fn compute_view_matrix(
    camera_position: Vector3<f32>,
    camera_rotation: Quaternion<f32>,
    head_position: Vector3<f32>,
    head_rotation: Quaternion<f32>,
) -> Matrix4<f32> {
    let camera_rotation: Matrix4<f32> = Matrix4::from(camera_rotation);
    let camera_translation = Matrix4::<f32>::from_translation(camera_position);
    let camera = (camera_translation * camera_rotation).invert().unwrap();

    let head_rotation = Matrix4::from(head_rotation);
    let head_offset = Matrix4::<f32>::from_translation(head_position);
    let head = (head_offset * head_rotation).invert().unwrap();

    head * camera
}

pub fn compute_view_matrix_from_render_context(
    render_context: &EngineRenderContext,
) -> Matrix4<f32> {
    compute_view_matrix(
        render_context.camera_offset,
        render_context.camera_rotation,
        render_context.head_offset,
        render_context.head_rotation,
    )
}

pub fn project_fast(
    projection_view: Matrix4<f32>,
    world_position: Vector3<f32>,
    screen_width: f32,
    screen_height: f32,
) -> Vector2<f32> {
    // Convert to homogeneous coordinates
    let homogenous_position =
        Vector4::new(world_position.x, world_position.y, world_position.z, 1.0);

    // Multiply by view and projection matrices
    let clip_space_position = projection_view * homogenous_position;

    // Perspective divide
    let normalized_device_coordinates = Vector3::new(
        clip_space_position.x / clip_space_position.w,
        clip_space_position.y / clip_space_position.w,
        clip_space_position.z / clip_space_position.w,
    );

    // Map to screen-space
    Vector2::new(
        (normalized_device_coordinates.x * 0.5 + 0.5) * screen_width,
        (1.0 - (normalized_device_coordinates.y * 0.5 + 0.5)) * screen_height,
    )
}

pub fn project(
    view: Matrix4<f32>,
    projection: Matrix4<f32>,
    world_position: Vector3<f32>,
    screen_width: f32,
    screen_height: f32,
) -> Vector2<f32> {
    project_fast(
        projection * view,
        world_position,
        screen_width,
        screen_height,
    )
}
