pub trait Geometry {
    fn draw(&self);
}

pub struct EmptyMesh;

impl Geometry for EmptyMesh {
    fn draw(&self) {
        // do nothing, the mesh is empty
    }
}
