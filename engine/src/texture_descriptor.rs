use crate::texture::Texture;
use std::rc::Rc;

pub struct FilePathTextureDescriptor {
    file_path: String,
    texture_format: Box<dyn crate::texture_format::TextureFormat>,
    texture: Option<Rc<Texture>>,
}

pub trait TextureDescriptor {
    fn initialize(&mut self, storage: &dyn crate::file_system::Storage);

    fn get_texture(&self) -> Rc<Texture>;
}

impl TextureDescriptor for FilePathTextureDescriptor {
    fn initialize(&mut self, storage: &dyn crate::file_system::Storage) {
        let file_system = storage.external_filesystem();
        if self.texture.is_none() {
            let image_buf = file_system.open_file(&self.file_path.to_owned());
            let texture = crate::texture::init2(&image_buf, &*self.texture_format);
            self.texture = Some(Rc::new(texture));
        }
    }

    fn get_texture(&self) -> Rc<Texture> {
        match &self.texture {
            None => {
                panic!("texture is not initialized");
            }
            Some(p) => p.clone(),
        }
    }
}

impl FilePathTextureDescriptor {
    pub fn new(
        file_path: String,
        texture_format: Box<dyn crate::texture_format::TextureFormat>,
    ) -> Box<dyn TextureDescriptor> {
        Box::new(FilePathTextureDescriptor {
            file_path,
            texture_format,
            texture: None,
        })
    }
}
