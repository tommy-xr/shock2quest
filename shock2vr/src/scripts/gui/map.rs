//! Flat-mode automap panel (`projects/flat-ui-panels.md` §5, `kOverlayMap 26`).
//!
//! The wide map MFD: `MAPBACK.PCX` frame (636x296 - the one panel spanning both
//! MFD slots), the current level's `PAGE001.PCX` page art, one bright
//! `P001R###.PCX` decal per *explored* map location (rects from `P001RA.BIN`),
//! and the player marker placed by the level's `MapRef` world->page transform.
//! Bound to a synthetic player-owned entity (no world object opens the map -
//! the original uses the BIOFULL MAP button / `M`); opened via
//! `Effect::ToggleMap` and hosted sticky (no walk-away close).
//!
//! Explored state lives in `QuestInfo` (per mission, persisted); the page data
//! rides on the entity as `RuntimePropMapData` (attached at mission init).
//!
//! Deferred (per the doc): minimap, nav markers/annotations, `MapText`
//! mouseover, the `page001a` Spatially-Aware variant, multi-page levels.

use cgmath::{Vector2, Vector3, vec2};
use dark::properties::{PropMapRef, PropPosition};
use shipyard::{EntityId, Get, IntoIter, UniqueView, View, World};

use crate::gui::{self, Gui, GuiComponent, GuiConfig, GuiCursor};
use crate::quest_info::QuestInfo;
use crate::runtime_props::RuntimePropMapData;
use crate::scripts::Effect;

/// `MAPBACK.PCX` frame size - the full-width panel rect {2,2}-{638,302} of the
/// original, anchored by the flat host at the left-MFD anchor.
const PANEL_W: f32 = 636.0;
const PANEL_H: f32 = 296.0;

/// `PAGE001.PCX` page-art size and its centered offset inside the frame.
const PAGE_W: f32 = 614.0;
const PAGE_H: f32 = 260.0;
const PAGE_X: f32 = (PANEL_W - PAGE_W) / 2.0;
const PAGE_Y: f32 = (PANEL_H - PAGE_H) / 2.0;

/// Player marker art (`Plrpip.pcx`) size, drawn centered on the position.
const MARKER_SIZE: f32 = 16.0;

pub struct MapGui;

#[derive(Clone, Debug, Default)]
pub struct MapGuiState {}

/// The map is read-only for now (close is the host's button / bare-view click /
/// `ToggleMap` again) - no clickable widgets, so no messages.
#[derive(Clone)]
pub enum MapGuiMsg {}

/// World->page mapping solved from the level's two `MapRef` scale markers
/// (`frame == -1`): each maps a world position to page pixels. Some level maps
/// are drawn rotated 90 degrees (the original's `m_rotatehack`), which swaps
/// which world axis feeds which page axis - detected here by picking the axis
/// assignment whose two scale factors are closest in magnitude (the page art is
/// uniformly scaled; the wrong assignment produces wildly mismatched scales).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapTransform {
    /// Page x/y fed by world z/x (rotated map) instead of x/z.
    swapped: bool,
    sx: f32,
    bx: f32,
    sy: f32,
    by: f32,
}

impl MapTransform {
    /// Solve from two (world (x, z), page (x, y)) reference pairs.
    pub fn solve(w1: (f32, f32), p1: (f32, f32), w2: (f32, f32), p2: (f32, f32)) -> Option<Self> {
        let fit = |a1: f32, out1: f32, a2: f32, out2: f32| -> Option<(f32, f32)> {
            let da = a2 - a1;
            if da.abs() < 1e-3 {
                return None;
            }
            let s = (out2 - out1) / da;
            Some((s, out1 - s * a1))
        };
        let direct = match (fit(w1.0, p1.0, w2.0, p2.0), fit(w1.1, p1.1, w2.1, p2.1)) {
            (Some((sx, bx)), Some((sy, by))) => Some(MapTransform {
                swapped: false,
                sx,
                bx,
                sy,
                by,
            }),
            _ => None,
        };
        let swapped = match (fit(w1.1, p1.0, w2.1, p2.0), fit(w1.0, p1.1, w2.0, p2.1)) {
            (Some((sx, bx)), Some((sy, by))) => Some(MapTransform {
                swapped: true,
                sx,
                bx,
                sy,
                by,
            }),
            _ => None,
        };
        // Prefer the assignment with the more uniform |scale| pair.
        let uniformity = |t: &MapTransform| (t.sx.abs() - t.sy.abs()).abs() / t.sx.abs().max(1e-6);
        match (direct, swapped) {
            (Some(d), Some(s)) => Some(if uniformity(&d) <= uniformity(&s) {
                d
            } else {
                s
            }),
            (Some(d), None) => Some(d),
            (None, Some(s)) => Some(s),
            (None, None) => None,
        }
    }

    /// Map a world (x, z) to page pixels.
    pub fn apply(&self, world_x: f32, world_z: f32) -> (f32, f32) {
        let (a, b) = if self.swapped {
            (world_z, world_x)
        } else {
            (world_x, world_z)
        };
        (self.sx * a + self.bx, self.sy * b + self.by)
    }
}

/// Solve the current level's transform from the world's `MapRef` scale markers
/// (`frame == -1`; they carry both the page point and a world position).
fn solve_transform_from_world(world: &World) -> Option<MapTransform> {
    let v_map_ref = world.borrow::<View<PropMapRef>>().ok()?;
    let v_pos = world.borrow::<View<PropPosition>>().ok()?;
    let mut markers = (&v_map_ref, &v_pos)
        .iter()
        .filter(|(map_ref, _)| map_ref.frame == -1)
        .map(|(map_ref, pos)| {
            (
                (pos.position.x, pos.position.z),
                (map_ref.x as f32, map_ref.y as f32),
            )
        });
    let (w1, p1) = markers.next()?;
    let (w2, p2) = markers.next()?;
    MapTransform::solve(w1, p1, w2, p2)
}

impl Gui<MapGuiState, MapGuiMsg> for MapGui {
    fn get_components(
        &self,
        _cursor: &Option<GuiCursor>,
        entity_id: EntityId,
        world: &World,
        _state: &MapGuiState,
    ) -> Vec<GuiComponent<MapGuiMsg>> {
        let mut components: Vec<GuiComponent<MapGuiMsg>> = vec![
            gui::image("mapback.pcx")
                .with_position(vec2(0.0, 0.0))
                .with_size(vec2(PANEL_W, PANEL_H)),
        ];

        let v_data = world.borrow::<View<RuntimePropMapData>>().unwrap();
        let Ok(data) = v_data.get(entity_id) else {
            return components;
        };
        if data.revealed_rects.is_empty() {
            // Level ships no automap page - the original's `nomap` art.
            components.push(
                gui::image("nomap.pcx")
                    .with_position(vec2((PANEL_W - 593.0) / 2.0, (PANEL_H - 281.0) / 2.0))
                    .with_size(vec2(593.0, 281.0)),
            );
            return components;
        }

        // Per-level art lives under `intrface/<LEVEL>/english/`.
        let level = data
            .mission
            .split('.')
            .next()
            .unwrap_or(&data.mission)
            .to_uppercase();
        components.push(
            gui::image(&format!("{level}/english/PAGE001.PCX"))
                .with_position(vec2(PAGE_X, PAGE_Y))
                .with_size(vec2(PAGE_W, PAGE_H)),
        );

        // One bright decal per explored location (rect + art indexed by the
        // room's MapLoc), in page space offset into the frame.
        let explored = world
            .borrow::<UniqueView<QuestInfo>>()
            .map(|q| q.explored_map_locations(&data.mission))
            .unwrap_or_default();
        for location in explored {
            let Some(rect) = usize::try_from(location)
                .ok()
                .and_then(|idx| data.revealed_rects.get(idx))
            else {
                continue;
            };
            components.push(
                gui::image(&format!("{level}/english/P001R{location:03}.PCX"))
                    .with_position(vec2(PAGE_X + rect.ul_x as f32, PAGE_Y + rect.ul_y as f32))
                    .with_size(vec2(rect.width() as f32, rect.height() as f32)),
            );
        }

        // Player marker: world position through the MapRef transform, clamped
        // onto the page. (Heading rotation - MapObjRotate - is deferred.)
        if let (Some(transform), Ok(player)) = (
            solve_transform_from_world(world),
            world.borrow::<UniqueView<crate::mission::PlayerInfo>>(),
        ) {
            let (px, py) = transform.apply(player.pos.x, player.pos.z);
            let px = px.clamp(0.0, PAGE_W);
            let py = py.clamp(0.0, PAGE_H);
            components.push(
                gui::image("plrpip.pcx")
                    .with_position(vec2(
                        PAGE_X + px - MARKER_SIZE / 2.0,
                        PAGE_Y + py - MARKER_SIZE / 2.0,
                    ))
                    .with_size(vec2(MARKER_SIZE, MARKER_SIZE)),
            );
        }

        components
    }

    fn get_config(&self) -> GuiConfig {
        GuiConfig {
            world_offset: Vector3::new(0.0, 0.0, -0.2),
            screen_size_in_pixels: Vector2::new(PANEL_W, PANEL_H),
        }
    }

    fn handle_msg(
        &self,
        _entity_id: EntityId,
        _world: &World,
        _state: &MapGuiState,
        msg: &MapGuiMsg,
    ) -> (MapGuiState, Effect) {
        match *msg {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real medsci1 scale markers (mission ids 1032/1034, `frame == -1`):
    /// world (-40.311, 32.874) -> page (536, 239) and world (44.685, -76.399)
    /// -> page (232, 10). medsci1's page art is drawn rotated (world z feeds
    /// page x), so the solver must pick the swapped assignment - the direct one
    /// produces wildly non-uniform scales (-3.58 vs 2.10).
    #[test]
    fn medsci1_transform_is_axis_swapped_and_exact() {
        let t = MapTransform::solve(
            (-40.311_17, 32.874_435),
            (536.0, 239.0),
            (44.685_417, -76.398_605),
            (232.0, 10.0),
        )
        .expect("two distinct markers must solve");
        assert!(t.swapped, "medsci1's map is rotated (z -> page x)");
        // Both markers must round-trip exactly.
        let (x1, y1) = t.apply(-40.311_17, 32.874_435);
        assert!((x1 - 536.0).abs() < 0.5 && (y1 - 239.0).abs() < 0.5);
        let (x2, y2) = t.apply(44.685_417, -76.398_605);
        assert!((x2 - 232.0).abs() < 0.5 && (y2 - 10.0).abs() < 0.5);
        // And the scales are near-uniform in magnitude.
        assert!((t.sx.abs() - t.sy.abs()).abs() / t.sx.abs() < 0.05);
    }

    #[test]
    fn degenerate_markers_do_not_solve() {
        // Identical world points can't span a transform.
        assert_eq!(
            MapTransform::solve((1.0, 2.0), (10.0, 20.0), (1.0, 2.0), (30.0, 40.0)),
            None
        );
    }

    #[test]
    fn unrotated_map_picks_the_direct_assignment() {
        // Synthetic level: page x = 2*wx + 100, page y = -2*wz + 200.
        let t = MapTransform::solve((0.0, 0.0), (100.0, 200.0), (50.0, -50.0), (200.0, 300.0))
            .expect("solvable");
        assert!(!t.swapped);
        let (px, py) = t.apply(10.0, 10.0);
        assert!((px - 120.0).abs() < 1e-3);
        assert!((py - 180.0).abs() < 1e-3);
    }
}
