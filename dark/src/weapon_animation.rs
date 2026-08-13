//! First-person weapon animations, as the 25th Anniversary Edition ships them.
//!
//! The remaster animates its viewmodels from **plain-text keyframe data**, not
//! from `.mc` motion files - which is why nothing in `res/motions` targets a
//! first-person arm. The data lives in the KEX layer's Squirrel scripts
//! (`sq_scripts/animations_weapons.nut`) as literal tables:
//!
//! ```text
//! ND.g_weaponAnimations["Pistol"]["raise"] <- ND.PointRigAnimation("raise", 30, 60,
//! {
//!     "gunPoint" : [
//!         { "frame":0,  "pos":[0.0, 0.0,-1.5], "rot":[45.0, 45.0, -45.0] },
//!         { "frame":15, "pos":[0.1, 0.2, 0.1], "rot":[15.0, -15.0, -60.0] },
//!     ],
//!     "joint1" : [ ... ],   // the pistol's slide
//! });
//! ```
//!
//! A track name is either `gunPoint` - the viewmodel as a whole - or `jointN`,
//! addressing the model's Nth sub-object. Those sub-objects are the moving parts
//! *and the hands* (`@s01_han`, `@s02_han`), so these clips animate the hand
//! rigidly, as translation and rotation. That is exactly what a leaf bone can
//! do, and is why draw/fire/reload need no finger rig.
//!
//! Positions are in SS2 units (divide by [`crate::SCALE_FACTOR`]); rotations are
//! Euler degrees. `events` fire at a frame - `muzzleFlash`, `eject`, `sound`.
//!
//! Only the subset of Squirrel these tables use is parsed, tolerantly: comments,
//! and the missing commas between keyframes that the shipped file contains.

use std::collections::HashMap;

use cgmath::{Vector3, vec3};

/// One authored key. `pos` is in SS2 units, `rot` Euler degrees.
#[derive(Debug, Clone, PartialEq)]
pub struct Keyframe {
    pub frame: f32,
    pub pos: Vector3<f32>,
    pub rot: Vector3<f32>,
    pub events: Vec<String>,
}

/// The keys for one track - `gunPoint`, or `jointN` for the Nth sub-object.
#[derive(Debug, Clone)]
pub struct Track {
    pub joint: String,
    pub keys: Vec<Keyframe>,
}

#[derive(Debug, Clone)]
pub struct WeaponAnimation {
    pub name: String,
    pub fps: f32,
    /// Authored length in frames.
    pub length: f32,
    pub tracks: Vec<Track>,
}

impl WeaponAnimation {
    pub fn duration_seconds(&self) -> f32 {
        if self.fps > 0.0 {
            self.length / self.fps
        } else {
            0.0
        }
    }

    pub fn track(&self, joint: &str) -> Option<&Track> {
        self.tracks.iter().find(|track| track.joint == joint)
    }

    /// The track's `(pos, rot)` at `frame`, linearly interpolated and clamped to
    /// the authored range.
    pub fn sample(&self, joint: &str, frame: f32) -> Option<(Vector3<f32>, Vector3<f32>)> {
        let keys = &self.track(joint)?.keys;
        let first = keys.first()?;
        let last = keys.last()?;

        if frame <= first.frame {
            return Some((first.pos, first.rot));
        }
        if frame >= last.frame {
            return Some((last.pos, last.rot));
        }

        // `<` not `<=`, so an exact hit on a repeated frame selects the *last*
        // key at that frame - authored data uses a duplicated frame to make a
        // value jump, and taking the first key would render the pre-jump value.
        let span = keys.windows(2).find(|pair| frame < pair[1].frame)?;
        let (a, b) = (&span[0], &span[1]);
        let width = b.frame - a.frame;
        // Coincident keys would divide by zero; the later key wins.
        let t = if width.abs() <= f32::EPSILON {
            1.0
        } else {
            (frame - a.frame) / width
        };

        Some((a.pos + (b.pos - a.pos) * t, a.rot + (b.rot - a.rot) * t))
    }

    /// Events firing in `(previous, current]`, so stepping the clip reports each
    /// event exactly once.
    ///
    /// The window is half-open at the start, so a caller must seed `previous`
    /// *below* the first frame - start a clip at `-1.0`, not `0.0`, or a
    /// frame-0 event (every `shoot` clip's `muzzleFlash`) never fires. Looping
    /// likewise needs two calls: to the end, then from below zero again.
    pub fn events_between(&self, previous: f32, current: f32) -> Vec<&str> {
        self.tracks
            .iter()
            .flat_map(|track| track.keys.iter())
            .filter(|key| key.frame > previous && key.frame <= current)
            .flat_map(|key| key.events.iter().map(String::as_str))
            .collect()
    }
}

/// Every animation, by weapon category then clip name (`shoot`, `reload`,
/// `raise`, `drop`, `idle1`). Categories match the weapon's name; `default`
/// is the fallback.
#[derive(Debug, Clone, Default)]
pub struct WeaponAnimations {
    pub by_category: HashMap<String, HashMap<String, WeaponAnimation>>,
}

impl WeaponAnimations {
    /// `category`'s clip, falling back to `default` - which is how the shipped
    /// data covers weapons with no bespoke animation.
    pub fn get(&self, category: &str, clip: &str) -> Option<&WeaponAnimation> {
        self.by_category
            .get(category)
            .and_then(|clips| clips.get(clip))
            .or_else(|| self.by_category.get("default")?.get(clip))
    }
}

/// Parses the subset of Squirrel `animations_weapons.nut` uses.
///
/// Anything unrecognized is skipped rather than fatal: this is third-party data
/// we do not control, and a future remaster patch adding syntax we do not model
/// should cost us one animation, not the whole file.
pub fn parse(source: &str) -> WeaponAnimations {
    let text = strip_comments(source);
    let bytes = text.as_bytes();
    let mut animations = WeaponAnimations::default();

    let mut at = 0usize;
    while let Some(found) = text[at..].find("g_weaponAnimations[") {
        let start = at + found + "g_weaponAnimations[".len();
        at = start;

        let Some((category, after_category)) = quoted(bytes, start) else {
            continue;
        };
        // A category declaration (`... <- {}`) has no second subscript.
        let Some(open) = next_non_space(bytes, after_category) else {
            continue;
        };
        if bytes.get(open) != Some(&b']') {
            continue;
        }
        let Some(second) = next_non_space(bytes, open + 1) else {
            continue;
        };
        if bytes.get(second) != Some(&b'[') {
            continue;
        }
        let Some((clip, after_clip)) = quoted(bytes, second + 1) else {
            continue;
        };

        // Bound every search below to this statement. Scanning to end-of-file
        // lets an entry we cannot parse latch onto the *next* clip's call and
        // table, filing it under the wrong key and then skipping the real
        // definition - one unsupported entry costing two animations plus a
        // corrupt one. The next declaration starts the next statement.
        let statement = text[after_clip..]
            .find("g_weaponAnimations[")
            .map(|found| after_clip + found)
            .unwrap_or(text.len());

        let Some(call) = text[after_clip..statement].find("PointRigAnimation") else {
            continue;
        };
        let call = after_clip + call + "PointRigAnimation".len();

        let Some(paren) = next_non_space(bytes, call) else {
            continue;
        };
        if bytes.get(paren) != Some(&b'(') {
            continue;
        }
        let Some((name, after_name)) = quoted(bytes, paren + 1) else {
            continue;
        };
        let Some((fps, after_fps)) = number_after_comma(bytes, after_name) else {
            continue;
        };
        let Some((length, after_length)) = number_after_comma(bytes, after_fps) else {
            continue;
        };

        let Some(table) = text[after_length..statement].find('{') else {
            continue;
        };
        let table = after_length + table;
        let Some(end) = matching_brace(bytes, table) else {
            continue;
        };

        let tracks = parse_tracks(&text[table + 1..end]);
        at = end;

        animations
            .by_category
            .entry(category)
            .or_default()
            .insert(
                clip,
                WeaponAnimation {
                    name,
                    fps,
                    length,
                    tracks,
                },
            );
    }

    animations
}

fn parse_tracks(body: &str) -> Vec<Track> {
    let bytes = body.as_bytes();
    let mut tracks = Vec::new();
    let mut at = 0usize;

    while let Some((joint, after_joint)) = next_quoted(bytes, at) {
        let Some(colon) = next_non_space(bytes, after_joint) else {
            break;
        };
        if bytes.get(colon) != Some(&b':') {
            at = after_joint;
            continue;
        }
        let Some(open) = next_non_space(bytes, colon + 1) else {
            break;
        };
        if bytes.get(open) != Some(&b'[') {
            at = colon + 1;
            continue;
        }
        let Some(close) = matching_bracket(bytes, open) else {
            break;
        };

        tracks.push(Track {
            joint: joint.to_owned(),
            keys: parse_keys(&body[open + 1..close]),
        });
        at = close + 1;
    }

    tracks
}

fn parse_keys(body: &str) -> Vec<Keyframe> {
    let bytes = body.as_bytes();
    let mut keys = Vec::new();
    let mut at = 0usize;

    // Keyframes are `{...}` records; the shipped file omits some separating
    // commas, so scan brace to brace rather than splitting on commas.
    while let Some(open) = body[at..].find('{').map(|found| at + found) {
        let Some(close) = matching_brace(bytes, open) else {
            break;
        };
        let record = &body[open + 1..close];
        at = close + 1;

        let Some(frame) = field_number(record, "frame") else {
            continue;
        };
        keys.push(Keyframe {
            frame,
            pos: field_vec3(record, "pos").unwrap_or_else(|| vec3(0.0, 0.0, 0.0)),
            rot: field_vec3(record, "rot").unwrap_or_else(|| vec3(0.0, 0.0, 0.0)),
            events: field_strings(record, "events"),
        });
    }

    keys.sort_by(|a, b| a.frame.total_cmp(&b.frame));
    keys
}

fn strip_comments(source: &str) -> String {
    source
        .lines()
        .map(|line| match line.find("//") {
            Some(at) => &line[..at],
            None => line,
        })
        .collect::<Vec<&str>>()
        .join("\n")
}

fn next_non_space(bytes: &[u8], from: usize) -> Option<usize> {
    (from..bytes.len()).find(|at| !bytes[*at].is_ascii_whitespace())
}

/// The string literal starting at the quote at or after `from`.
fn quoted(bytes: &[u8], from: usize) -> Option<(String, usize)> {
    let open = next_non_space(bytes, from)?;
    if bytes.get(open) != Some(&b'"') {
        return None;
    }
    let close = (open + 1..bytes.len()).find(|at| bytes[*at] == b'"')?;
    Some((
        String::from_utf8_lossy(&bytes[open + 1..close]).into_owned(),
        close + 1,
    ))
}

/// The next string literal anywhere at or after `from`.
fn next_quoted(bytes: &[u8], from: usize) -> Option<(String, usize)> {
    let open = (from..bytes.len()).find(|at| bytes[*at] == b'"')?;
    let close = (open + 1..bytes.len()).find(|at| bytes[*at] == b'"')?;
    Some((
        String::from_utf8_lossy(&bytes[open + 1..close]).into_owned(),
        close + 1,
    ))
}

fn number_after_comma(bytes: &[u8], from: usize) -> Option<(f32, usize)> {
    let comma = next_non_space(bytes, from)?;
    if bytes.get(comma) != Some(&b',') {
        return None;
    }
    let start = next_non_space(bytes, comma + 1)?;
    let end = (start..bytes.len())
        .find(|at| !matches!(bytes[*at], b'0'..=b'9' | b'.' | b'-' | b'+'))
        .unwrap_or(bytes.len());
    let text = std::str::from_utf8(&bytes[start..end]).ok()?;
    Some((text.parse().ok()?, end))
}

/// The value of `"field" : ...` inside one record.
fn field_body(record: &str, field: &str) -> Option<usize> {
    let needle = format!("\"{field}\"");
    let at = record.find(&needle)? + needle.len();
    let colon = next_non_space(record.as_bytes(), at)?;
    if record.as_bytes().get(colon) != Some(&b':') {
        return None;
    }
    next_non_space(record.as_bytes(), colon + 1)
}

fn field_number(record: &str, field: &str) -> Option<f32> {
    let start = field_body(record, field)?;
    let bytes = record.as_bytes();
    let end = (start..bytes.len())
        .find(|at| !matches!(bytes[*at], b'0'..=b'9' | b'.' | b'-' | b'+'))
        .unwrap_or(bytes.len());
    record[start..end].parse().ok()
}

fn field_vec3(record: &str, field: &str) -> Option<Vector3<f32>> {
    let start = field_body(record, field)?;
    if record.as_bytes().get(start) != Some(&b'[') {
        return None;
    }
    let close = matching_bracket(record.as_bytes(), start)?;
    let parts = record[start + 1..close]
        .split(',')
        .filter_map(|part| part.trim().parse::<f32>().ok())
        .collect::<Vec<f32>>();
    match parts.len() {
        3 => Some(vec3(parts[0], parts[1], parts[2])),
        _ => None,
    }
}

fn field_strings(record: &str, field: &str) -> Vec<String> {
    let Some(start) = field_body(record, field) else {
        return Vec::new();
    };
    if record.as_bytes().get(start) != Some(&b'[') {
        return Vec::new();
    }
    let Some(close) = matching_bracket(record.as_bytes(), start) else {
        return Vec::new();
    };
    record[start + 1..close]
        .split(',')
        .filter_map(|part| {
            let trimmed = part.trim().trim_matches('"').trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        })
        .collect()
}

fn matching(bytes: &[u8], from: usize, open: u8, close: u8) -> Option<usize> {
    if bytes.get(from) != Some(&open) {
        return None;
    }
    let mut depth = 0usize;
    for at in from..bytes.len() {
        if bytes[at] == open {
            depth += 1;
        } else if bytes[at] == close {
            depth -= 1;
            if depth == 0 {
                return Some(at);
            }
        }
    }
    None
}

fn matching_brace(bytes: &[u8], from: usize) -> Option<usize> {
    matching(bytes, from, b'{', b'}')
}

fn matching_bracket(bytes: &[u8], from: usize) -> Option<usize> {
    matching(bytes, from, b'[', b']')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verbatim from the shipped `animations_weapons.nut`, including the two
    /// things that break a naive parser: a `//` comment, and the **missing
    /// commas** between the frame-26 and frame-28 keys.
    const PISTOL_RAISE: &str = r#"
ND.g_weaponAnimations["Pistol"] <- {};
ND.g_weaponAnimations["Pistol"]["raise"] <- ND.PointRigAnimation("raise", 30, 60,
{
	// Weapon
	"gunPoint" : [
		{ "frame":0, "pos":[0.0, 0.0,-1.5], "rot":[45.0, 45.0, -45.0]},
		{ "frame":15, "pos":[0.1, 0.2, 0.1], "rot":[15.0, -15.0, -60.0] },
		{ "frame":26, "pos":[0.1, 0.2, 0.1], "rot":[15.0, -15.0, -60.0] }

		{ "frame":28, "pos":[0.15, 0.2, 0.1], "rot":[0.0, 0.0, -45.0] }
		{ "frame":52, "pos":[0.0, 0.0, 0.0], "rot":[0.0, 0.0, 0.0] },
	],

	// Slide
	"joint1" : [
		{ "frame":0, "pos":[0.0, 0.0,0.0], "rot":[0.0, 0.0, 0.0] },
		{ "frame":16, "pos":[-0.20, 0.0,0.0], "rot":[0.0, 0.0, 0.0] },
		{ "frame":26, "pos":[0.0, 0.0,0.0], "rot":[0.0, 0.0, 0.0] },
	],
});
"#;

    fn pistol_raise() -> WeaponAnimation {
        parse(PISTOL_RAISE)
            .by_category
            .remove("Pistol")
            .expect("Pistol category")
            .remove("raise")
            .expect("raise clip")
    }

    #[test]
    fn parses_the_clip_header() {
        let raise = pistol_raise();
        assert_eq!(raise.name, "raise");
        assert_eq!(raise.fps, 30.0);
        assert_eq!(raise.length, 60.0);
        assert_eq!(raise.duration_seconds(), 2.0);
    }

    /// The comma between frames 26 and 28 is missing in the shipped file, so a
    /// comma-splitting parser silently loses keys.
    #[test]
    fn keyframes_survive_missing_commas() {
        let raise = pistol_raise();
        let gun = raise.track("gunPoint").expect("gunPoint track");
        assert_eq!(gun.keys.len(), 5);
        assert_eq!(
            gun.keys.iter().map(|key| key.frame).collect::<Vec<f32>>(),
            vec![0.0, 15.0, 26.0, 28.0, 52.0]
        );
    }

    /// The slide pull: back by frame 16, returned by 26.
    #[test]
    fn samples_the_slide_pull() {
        let raise = pistol_raise();

        let (back, _) = raise.sample("joint1", 16.0).expect("slide at 16");
        assert!((back.x - -0.20).abs() < 1e-5, "slide should be back: {back:?}");

        let (mid, _) = raise.sample("joint1", 8.0).expect("slide at 8");
        assert!(
            mid.x < 0.0 && mid.x > -0.20,
            "slide should be part way back: {mid:?}"
        );

        let (home, _) = raise.sample("joint1", 26.0).expect("slide at 26");
        assert!((home.x).abs() < 1e-5, "slide should be home: {home:?}");
    }

    #[test]
    fn sampling_clamps_outside_the_authored_range() {
        let raise = pistol_raise();
        let (before, _) = raise.sample("gunPoint", -5.0).expect("before start");
        let (after, _) = raise.sample("gunPoint", 999.0).expect("after end");
        assert_eq!(before.z, -1.5);
        assert_eq!(after, vec3(0.0, 0.0, 0.0));
    }

    #[test]
    fn events_fire_once_per_frame_window() {
        let source = r#"
ND.g_weaponAnimations["default"]["shoot"] <- ND.PointRigAnimation("shoot", 30, 60,
{
	"gunPoint" : [
		{ "frame":0, "pos":[0.0, 0.0, 0.0], "rot":[0.0, 0.0, 0.0], "events": ["muzzleFlash", "eject"]},
		{ "frame":12, "pos":[0.0, 0.0, 0.0], "rot":[0.0, 0.0, 0.0] },
	],
});
"#;
        let shoot = parse(source).get("default", "shoot").cloned().expect("shoot");
        assert_eq!(shoot.events_between(-1.0, 0.0), vec!["muzzleFlash", "eject"]);
        // Already consumed - stepping past must not repeat it.
        assert!(shoot.events_between(0.0, 12.0).is_empty());
    }

    /// An entry we cannot parse must cost only itself. Before this was bounded,
    /// the unparsable entry swallowed the *next* clip's call and table.
    #[test]
    fn an_unparsable_entry_does_not_consume_the_next() {
        let source = r#"
ND.g_weaponAnimations["Pistol"]["broken"] <- ND.SomethingElse();
ND.g_weaponAnimations["Pistol"]["shoot"] <- ND.PointRigAnimation("shoot", 30, 42,
{
	"gunPoint" : [ { "frame":0, "pos":[7.0, 0.0, 0.0], "rot":[0.0, 0.0, 0.0] } ],
});
"#;
        let animations = parse(source);
        let shoot = animations.get("Pistol", "shoot").expect("shoot survives");
        assert_eq!(shoot.length, 42.0);
        assert_eq!(shoot.sample("gunPoint", 0.0).expect("sample").0.x, 7.0);
        assert!(animations.get("Pistol", "broken").is_none());
    }

    /// Authored data repeats a frame to make a value jump; the later key wins.
    #[test]
    fn a_repeated_frame_takes_the_later_key() {
        let source = r#"
ND.g_weaponAnimations["default"]["shoot"] <- ND.PointRigAnimation("shoot", 30, 10,
{
	"gunPoint" : [
		{ "frame":0, "pos":[0.0, 0.0, 0.0], "rot":[0.0, 0.0, 0.0] },
		{ "frame":5, "pos":[1.0, 0.0, 0.0], "rot":[0.0, 0.0, 0.0] },
		{ "frame":5, "pos":[9.0, 0.0, 0.0], "rot":[0.0, 0.0, 0.0] },
		{ "frame":10, "pos":[9.0, 0.0, 0.0], "rot":[0.0, 0.0, 0.0] },
	],
});
"#;
        let shoot = parse(source).get("default", "shoot").cloned().expect("shoot");
        let (at_five, _) = shoot.sample("gunPoint", 5.0).expect("sample at 5");
        assert_eq!(at_five.x, 9.0, "the later key at frame 5 should win");
    }

    /// A weapon with no bespoke clip falls back to `default`, which is how the
    /// shipped data covers most of the roster.
    #[test]
    fn unknown_category_falls_back_to_default() {
        let source = r#"
ND.g_weaponAnimations["default"]["reload"] <- ND.PointRigAnimation("reload", 30, 60,
{
	"gunPoint" : [ { "frame":0, "pos":[1.0, 0.0, 0.0], "rot":[0.0, 0.0, 0.0] } ],
});
"#;
        let animations = parse(source);
        assert!(animations.get("Fusion Cannon", "reload").is_some());
        assert!(animations.get("Fusion Cannon", "shoot").is_none());
    }
}
