use std::rc::Rc;

use engine::texture::Texture;

pub struct BitmapAnimation {
    frames: Vec<Rc<Texture>>,
}

pub enum FrameOptions {
    Clamp,
    Wrap,
}

impl BitmapAnimation {
    pub fn new(frames: Vec<Rc<Texture>>) -> BitmapAnimation {
        BitmapAnimation { frames }
    }

    pub fn total_frames(&self) -> usize {
        self.frames.len()
    }

    pub fn get_frame(&self, frame: usize, opts: FrameOptions) -> Option<Rc<Texture>> {
        let frame = match opts {
            FrameOptions::Clamp => frame.min(self.frames.len() - 1),
            FrameOptions::Wrap => frame % self.frames.len(),
        };
        self.frames.get(frame).cloned()
    }
}
