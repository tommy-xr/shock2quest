//! Retail ND_MeshAnimated: discrete model frames selected by simulation time.
//! This replaces only the drawn mesh. The original model still owns collision,
//! damage proxies and joint tweqs; changing it through ChangeModel would rebuild
//! those systems twenty times per second.
use crate::mission::mission_core::{GlobalEntityMetadata, GlobalTemplateHierarchy};
use dark::{importers::MODELS_IMPORTER, model::Model, properties::PropTemplateId};
use engine::assets::asset_cache::AssetCache;
use shipyard::{EntityId, Get, UniqueView, View, World};
use std::{rc::Rc, time::Duration};

struct Sequence {
    prefix: &'static str,
    frames: u32,
    frame_rate: u32,
}

impl Sequence {
    fn frame(&self, time: Duration) -> u32 {
        // Integer nanoseconds avoid rounding a long-running clock to f32 frames.
        ((time.as_nanos() * u128::from(self.frame_rate) / 1_000_000_000) % u128::from(self.frames))
            as u32
    }
}

const GRUB: Sequence = Sequence {
    prefix: "NDgrub",
    frames: 20,
    frame_rate: 20,
};

pub fn model(
    world: &World,
    assets: &mut AssetCache,
    entity: EntityId,
    frozen_time: Option<Duration>,
) -> Option<(String, Rc<Model>)> {
    let templates = world.borrow::<View<PropTemplateId>>().ok()?;
    let metadata = world.borrow::<UniqueView<GlobalEntityMetadata>>().ok()?;
    let hierarchy = world.borrow::<UniqueView<GlobalTemplateHierarchy>>().ok()?;
    let template = templates.get(entity).ok()?.template_id;
    // Both registrations are active in object_grub.nut. In particular, do not
    // apply the commented corpse/debris changes from dbmods/ND-grub.dml.
    if !["grub", "grub rotatable"].iter().any(|name| {
        metadata
            .0
            .get(*name)
            .is_some_and(|parent| hierarchy.is_or_descends_from(template, parent.template_id))
    }) {
        return None;
    }
    let time = world.borrow::<UniqueView<crate::time::Time>>().ok()?;
    let name = format!(
        "{}{}",
        GRUB.prefix,
        GRUB.frame(frozen_time.unwrap_or(time.total))
    );
    // AssetCache shares the twenty immutable meshes across every grub and
    // reuses them on subsequent cycles. Missing remaster art keeps the legacy
    // model visible instead of altering the entity or its saved properties.
    let model = assets.get_opt(&MODELS_IMPORTER, &format!("{name}.bin"))?;
    Some((name, model))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grub_has_twenty_frames_per_second_at_exact_boundaries() {
        for cycle in [0, 1, 3600] {
            for frame in 0..20 {
                let start = Duration::from_millis(cycle * 1000 + frame * 50);
                assert_eq!(GRUB.frame(start), frame as u32);
                assert_eq!(GRUB.frame(start + Duration::from_millis(49)), frame as u32);
            }
        }
    }
}
