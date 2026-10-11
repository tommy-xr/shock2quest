pub trait Geometry {
    fn draw(&self);
    fn has_morph(&self) -> bool {
        false
    }
    fn draw_with_morph(&self, _weights: [f32; 3]) {
        self.draw();
    }
}

pub struct EmptyMesh;

impl Geometry for EmptyMesh {
    fn draw(&self) {
        // do nothing, the mesh is empty
    }
}
