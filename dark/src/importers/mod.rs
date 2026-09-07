mod animation_clip_importer;
mod audio_importer;
mod bitmap_animation_importer;
mod font_importer;
mod glb_model_importer;
mod model_importer;
mod motiondb_importer;
mod rect_list_importer;
mod skeleton_importer;
mod song_importer;
mod strings_importer;
mod text_importer;
mod texture_importer;

pub use animation_clip_importer::*;
pub use audio_importer::*;
pub use bitmap_animation_importer::*;
pub use font_importer::*;
pub use glb_model_importer::*;
pub use model_importer::*;
pub use motiondb_importer::*;
pub use rect_list_importer::*;
pub use skeleton_importer::*;
pub use song_importer::*;
pub use strings_importer::*;
pub use text_importer::*;
pub use texture_importer::*;

#[cfg(test)]
mod tests {
    use super::{MAP_POSITION_IMPORTER, UI_LAYOUT_IMPORTER};

    #[test]
    fn map_and_ui_rects_share_one_importer() {
        let map_importer = std::ptr::from_ref(&*MAP_POSITION_IMPORTER) as *const ();
        let ui_importer = std::ptr::from_ref(&*UI_LAYOUT_IMPORTER) as *const ();

        assert_eq!(map_importer, ui_importer);
    }
}
