//! Flat-mode automap panel (`projects/flat-ui-panels.md` §5, `kOverlayMap 26`).
//!
//! The wide map MFD: `MAPBACK.PCX` frame (636x296 - the one panel spanning both
//! MFD slots), the current level's `PAGE001.PCX` page art, one dim
//! `P001X###.PCX` decal per *explored* map location (rects from `P001XA.BIN`)
//! with the bright `P001R###.PCX` art only for the location the player is
//! currently in, and the player marker placed by the level's `MapRef`
//! world->page transform (per-frame markers relocate multi-story areas into
//! the page's inset boxes).
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

/// `PAGE001.PCX` page-art size (shared with the world map renderer via
/// `dark::map`) and its fixed offset inside the frame - the original engine
/// draws the page at (10, 8), leaving the frame's bottom strip clear.
const PAGE_W: f32 = dark::map::PAGE_WIDTH;
const PAGE_H: f32 = dark::map::PAGE_HEIGHT;
const PAGE_X: f32 = 10.0;
const PAGE_Y: f32 = 8.0;

/// Player marker art (`Plrpip.pcx`) size, drawn centered on the position.
const MARKER_SIZE: f32 = 16.0;

pub struct MapGui;

#[derive(Clone, Debug, Default)]
pub struct MapGuiState {}

/// The map is read-only for now (close is the host's button / bare-view click /
/// `ToggleMap` again) - no clickable widgets, so no messages.
#[derive(Clone)]
pub enum MapGuiMsg {}

/// KNOWN LIMITATION (xreview, both engines): the rotate-hack detection below is
/// a heuristic - a level whose page mapping is legitimately non-uniform in
/// scale (or near-tied between assignments) could get a mirrored/mis-placed
/// player marker (cosmetic; decals are unaffected). medsci1 is verified from
/// mission data; verify other decks' markers visually before trusting them
/// (follow-up tracked on the PR).
///
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

    /// Map a world-space delta (dx, dz) to a page-pixel delta - scale only, no
    /// translation. Used to place positions relative to a per-frame marker.
    pub fn apply_delta(&self, world_dx: f32, world_dz: f32) -> (f32, f32) {
        let (a, b) = if self.swapped {
            (world_dz, world_dx)
        } else {
            (world_dx, world_dz)
        };
        (self.sx * a, self.sy * b)
    }
}

/// One `MapRef` marker: a page-pixel anchor tied to a world position.
/// `frame == -1` markers are the level's two global scale references; a
/// `frame >= 0` marker relocates one map location - typically a multi-story
/// area drawn in one of the page's inset boxes - so positions inside that
/// location are placed relative to it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapRefMarker {
    pub frame: i32,
    pub world: (f32, f32),
    pub page: (f32, f32),
}

/// Place the player pip in page pixels, following the original engine's
/// resolution order: if a `MapRef` marker exists for the player's current map
/// location, place relative to it (using the global transform's scale);
/// otherwise fall back to the global affine solved from the `frame == -1`
/// scale markers.
pub fn place_player_pip(
    markers: &[MapRefMarker],
    current_location: Option<i32>,
    world_pos: (f32, f32),
) -> Option<(f32, f32)> {
    let mut scale_markers = markers.iter().filter(|m| m.frame == -1);
    let (m1, m2) = (scale_markers.next()?, scale_markers.next()?);
    let transform = MapTransform::solve(m1.world, m1.page, m2.world, m2.page)?;
    if let Some(marker) = current_location.and_then(|loc| markers.iter().find(|m| m.frame == loc)) {
        let (dx, dy) =
            transform.apply_delta(world_pos.0 - marker.world.0, world_pos.1 - marker.world.1);
        return Some((marker.page.0 + dx, marker.page.1 + dy));
    }
    Some(transform.apply(world_pos.0, world_pos.1))
}

/// Collect the level's `MapRef` markers (each carries a page point, a frame,
/// and - via its entity - a world position).
fn collect_markers(world: &World) -> Vec<MapRefMarker> {
    let Ok(v_map_ref) = world.borrow::<View<PropMapRef>>() else {
        return Vec::new();
    };
    let Ok(v_pos) = world.borrow::<View<PropPosition>>() else {
        return Vec::new();
    };
    (&v_map_ref, &v_pos)
        .iter()
        .map(|(map_ref, pos)| MapRefMarker {
            frame: map_ref.frame,
            world: (pos.position.x, pos.position.z),
            page: (map_ref.x as f32, map_ref.y as f32),
        })
        .collect()
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
            // Level ships no automap page - the original's `nomap` art, at the
            // same fixed page offset inside the frame.
            components.push(
                gui::image("nomap.pcx")
                    .with_position(vec2(PAGE_X, PAGE_Y))
                    .with_size(vec2(593.0, 281.0)),
            );
            return components;
        }

        // Per-level art lives under `intrface/<LEVEL>/english/`.
        let level = data.mission.split('.').next().unwrap_or(&data.mission);
        components.push(
            gui::image(&dark::map::page_art_path(level))
                .with_position(vec2(PAGE_X, PAGE_Y))
                .with_size(vec2(PAGE_W, PAGE_H)),
        );

        // The location the player is currently in (the mapped room the room
        // sensors last placed them in) draws bright; the rest draw dim.
        let current_location = world
            .borrow::<UniqueView<crate::mission::PlayerMapLocation>>()
            .ok()
            .and_then(|current| current.0);

        // One dim `X` decal per explored location (rect + art indexed by the
        // room's MapLoc), with the bright `R` art on top only for the current
        // location - original engine behavior.
        let explored = world
            .borrow::<UniqueView<QuestInfo>>()
            .map(|q| q.explored_map_locations(&data.mission))
            .unwrap_or_default();
        for location in explored {
            let Some(idx) = usize::try_from(location).ok() else {
                continue;
            };
            if let Some(rect) = data.explored_rects.get(idx) {
                components.push(
                    gui::image(&dark::map::explored_decal_path(level, location))
                        .with_position(vec2(PAGE_X + rect.ul_x as f32, PAGE_Y + rect.ul_y as f32))
                        .with_size(vec2(rect.width() as f32, rect.height() as f32)),
                );
            }
            if Some(location) == current_location {
                if let Some(rect) = data.revealed_rects.get(idx) {
                    components.push(
                        gui::image(&dark::map::revealed_decal_path(level, location))
                            .with_position(vec2(
                                PAGE_X + rect.ul_x as f32,
                                PAGE_Y + rect.ul_y as f32,
                            ))
                            .with_size(vec2(rect.width() as f32, rect.height() as f32)),
                    );
                }
            }
        }

        // Player marker: world position through the MapRef markers (per-frame
        // marker for the current location if one exists, else the global
        // affine), clamped onto the page as a last resort. (Heading rotation -
        // MapObjRotate - is deferred.)
        let pip = world
            .borrow::<UniqueView<crate::mission::PlayerInfo>>()
            .ok()
            .and_then(|player| {
                place_player_pip(
                    &collect_markers(world),
                    current_location,
                    (player.pos.x, player.pos.z),
                )
            });
        if let Some((px, py)) = pip {
            // Clamp the marker fully inside the page art.
            let px = px.clamp(MARKER_SIZE / 2.0, PAGE_W - MARKER_SIZE / 2.0);
            let py = py.clamp(MARKER_SIZE / 2.0, PAGE_H - MARKER_SIZE / 2.0);
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

    /// All four real medsci1 `MapRef` markers, decoded from the shipped
    /// mission data: the two `frame == -1` global scale markers (ids
    /// 1032/1034) plus the per-frame markers for map locations 0 and 2 - the
    /// two "INSET LOWER LEVEL" boxes on the page art.
    fn medsci1_markers() -> Vec<MapRefMarker> {
        vec![
            MapRefMarker {
                frame: -1,
                world: (-40.311_17, 32.874_435),
                page: (536.0, 239.0),
            },
            MapRefMarker {
                frame: -1,
                world: (44.685_417, -76.398_605),
                page: (232.0, 10.0),
            },
            MapRefMarker {
                frame: 0,
                world: (11.585_943, 7.603_475),
                page: (92.0, 30.0),
            },
            MapRefMarker {
                frame: 2,
                world: (-19.003_445, -54.242_805),
                page: (72.0, 127.0),
            },
        ]
    }

    /// A player inside a location that has a per-frame `MapRef` marker must be
    /// placed relative to that marker - which relocates them into the page's
    /// inset box - NOT through the global affine (which would put the pip at
    /// the upper level's drawing of the same world x/z). Real medsci1 data:
    /// map location 2's inset rect is LTRB (25, 100, 144, 169) in P001RA.BIN.
    #[test]
    fn pip_uses_per_frame_marker_for_inset_locations() {
        let markers = medsci1_markers();
        // Standing exactly at the frame-2 marker's world position.
        let (px, py) = place_player_pip(&markers, Some(2), (-19.003_445, -54.242_805))
            .expect("markers must place the pip");
        assert!((px - 72.0).abs() < 0.5 && (py - 127.0).abs() < 0.5);
        // The global affine would have placed it far away (in the upper
        // level's drawing of that world x/z) - the relocation matters.
        let (gx, gy) = place_player_pip(&markers, None, (-19.003_445, -54.242_805)).unwrap();
        assert!(((px - gx).abs() + (py - gy).abs()) > 50.0);
        // A position a few world units into the room stays inside the
        // location's inset rect, LTRB (25, 100, 144, 169).
        let (nx, ny) = place_player_pip(&markers, Some(2), (-22.0, -58.0)).unwrap();
        assert!((25.0..=144.0).contains(&nx), "pip x {nx} outside inset");
        assert!((100.0..=169.0).contains(&ny), "pip y {ny} outside inset");
        // Same for map location 0's marker (inset rect LTRB (26, 30, 93, 90)).
        let (fx, fy) = place_player_pip(&markers, Some(0), (11.585_943, 7.603_475)).unwrap();
        assert!((fx - 92.0).abs() < 0.5 && (fy - 30.0).abs() < 0.5);
    }

    /// A location with no per-frame marker falls back to the global affine
    /// solved from the two `frame == -1` scale markers.
    #[test]
    fn pip_falls_back_to_global_affine_without_per_frame_marker() {
        let markers = medsci1_markers();
        let t = MapTransform::solve(
            markers[0].world,
            markers[0].page,
            markers[1].world,
            markers[1].page,
        )
        .unwrap();
        for current in [None, Some(4), Some(9)] {
            let (px, py) = place_player_pip(&markers, current, (10.0, -20.0)).unwrap();
            let (ex, ey) = t.apply(10.0, -20.0);
            assert!((px - ex).abs() < 1e-3 && (py - ey).abs() < 1e-3);
        }
    }

    #[test]
    fn pip_needs_two_scale_markers() {
        let mut markers = medsci1_markers();
        markers.remove(0);
        assert_eq!(place_player_pip(&markers, None, (0.0, 0.0)), None);
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
