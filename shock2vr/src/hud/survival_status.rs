//! A derived readout published by the Survive director. The director owns the
//! saved schedule; these world screens never inspect or deserialize script state.
use std::rc::Rc;

use cgmath::{Deg, Matrix4, vec2};
use engine::{
    assets::asset_cache::AssetCache,
    scene::{SceneObject, SceneObjectDebugTag},
};
use shipyard::{Unique, UniqueView, World};

use crate::ui::{HAlign, Rect, UiCanvas, VAlign};

#[derive(Clone, Debug, PartialEq, Unique)]
pub enum SurvivalStatus {
    Rest {
        next_wave: u32,
        seconds_remaining: f32,
        progress: f32,
    },
    Active {
        wave: u32,
        active: u32,
        remaining: u32,
        total: u32,
    },
    Complete,
    Failed {
        wave: u32,
    },
}

impl SurvivalStatus {
    pub(crate) fn progress(&self) -> f32 {
        match *self {
            Self::Rest { progress, .. } => progress.clamp(0.0, 1.0),
            Self::Active {
                remaining, total, ..
            } => total.saturating_sub(remaining) as f32 / total.max(1) as f32,
            Self::Complete => 1.0,
            Self::Failed { .. } => 0.0,
        }
    }

    fn canvas(&self) -> UiCanvas {
        let mut canvas = UiCanvas::new(vec2(360.0, 220.0));
        canvas.fill(Rect::new(0.0, 0.0, 360.0, 220.0), [35, 55, 59]);
        canvas.fill(Rect::new(5.0, 5.0, 350.0, 210.0), [3, 13, 16]);
        let mut text = |y, h, label: &str, size| {
            canvas.text(
                Rect::new(18.0, y, 324.0, h),
                label,
                crate::ui::TITLE_FONT,
                size,
                HAlign::Center,
                VAlign::Middle,
            );
        };
        text(12.0, 22.0, "SURVIVE / EARTH", 17.0);
        match *self {
            Self::Rest {
                next_wave,
                seconds_remaining,
                ..
            } => {
                text(46.0, 30.0, &format!("WAVE {next_wave} / REST"), 25.0);
                let seconds = seconds_remaining.max(0.0).ceil() as u32;
                text(
                    82.0,
                    48.0,
                    &format!("{}:{:02}", seconds / 60, seconds % 60),
                    44.0,
                );
                text(137.0, 24.0, "UNTIL NEXT WAVE", 18.0);
            }
            Self::Active {
                wave,
                active,
                remaining,
                ..
            } => {
                text(46.0, 32.0, &format!("WAVE {wave} / ACTIVE"), 25.0);
                text(88.0, 29.0, &format!("ENEMIES ACTIVE: {active}"), 21.0);
                text(
                    127.0,
                    29.0,
                    &format!("ENEMIES REMAINING: {remaining}"),
                    21.0,
                );
            }
            Self::Complete => {
                text(55.0, 40.0, "CONTAINMENT COMPLETE", 23.0);
                text(118.0, 30.0, "USE READY FOR ENDLESS", 19.0);
            }
            Self::Failed { wave } => {
                text(55.0, 40.0, "CONTAINMENT LOST", 25.0);
                text(118.0, 30.0, &format!("WAVE {wave}"), 23.0);
            }
        }
        canvas.fill(Rect::new(18.0, 177.0, 324.0, 22.0), [24, 59, 58]);
        canvas.fill(
            Rect::new(21.0, 180.0, 318.0 * self.progress(), 16.0),
            [0, 218, 164],
        );
        canvas
    }
}

/// Above a payphone on each floor, with the face just in front of its wall.
/// Placement and canvas are shared by desktop and VR; these are ordinary
/// depth-tested world objects, never a per-eye overlay.
const SCREENS: [([f32; 3], f32); 2] = [
    ([21.5558, 4.8, 12.32], 180.0),
    ([46.7478, 23.8, 20.07], 0.0),
];

pub(crate) fn render(world: &World, assets: &mut AssetCache) -> Vec<SceneObject> {
    let Ok(status) = world.borrow::<UniqueView<SurvivalStatus>>() else {
        return Vec::new();
    };
    let canvas = status.canvas();
    SCREENS
        .into_iter()
        .enumerate()
        .flat_map(|(floor, (position, yaw))| {
            let root = Matrix4::from_translation(position.into())
                * Matrix4::from_angle_y(Deg(yaw))
                * Matrix4::from_nonuniform_scale(2.7, 1.65, 1.0);
            let tag = Rc::new(SceneObjectDebugTag {
                source: Some("survival_status_screen".into()),
                name: Some(
                    if floor == 0 {
                        "Subway status"
                    } else {
                        "Street status"
                    }
                    .into(),
                ),
                entity_id: None,
                model: None,
            });
            let mut objects = canvas.render_world_space(assets, root, None, None, 0.001);
            for object in &mut objects {
                object.set_debug_tag(Some(tag.clone()));
            }
            objects
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_counts_defeated_enemies_and_elapsed_rest() {
        assert_eq!(
            SurvivalStatus::Active {
                wave: 3,
                active: 4,
                remaining: 9,
                total: 12
            }
            .progress(),
            0.25
        );
        assert_eq!(
            SurvivalStatus::Rest {
                next_wave: 4,
                seconds_remaining: 45.0,
                progress: 0.25
            }
            .progress(),
            0.25
        );
        assert_eq!(SurvivalStatus::Complete.progress(), 1.0);
    }
}
