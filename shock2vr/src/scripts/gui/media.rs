//! Audio-log / email reader MFD (`projects/flat-ui-panels.md` §1).
//!
//! The flat-mode reader panel matching the original game's email/log overlay: a
//! `LOG.PCX` backdrop with the sender portrait, deck icon, header line and a word-wrapped,
//! scrollable transcript. It is bound to the frobbed log-disc entity (the flat
//! host's single-slot MFD), and reads its presentation strings from
//! `RuntimePropLogData` - attached by the `Effect::CollectLog` handler when the
//! disc is frobbed (that handler also records the log into the persistent
//! `QuestInfo` collection and plays its audio). Deliberate deviation from the
//! original's destroy-on-pickup: the disc survives so the reader stays bound and
//! the code is readable in-fiction (research gap #6).

use cgmath::{Vector2, Vector3, vec2};
use dark::properties::PropLog;
use engine::audio::AudioHandle;
use shipyard::{EntityId, Get, View, World};

use crate::gui::{self, Gui, GuiComponent, GuiConfig, GuiCursor};
use crate::runtime_props::RuntimePropLogData;
use crate::scripts::{Effect, MessagePayload, script_util::send_to_all_switch_links};

const PANEL_W: f32 = 188.0;
const PANEL_H: f32 = 296.0;

/// Layout matching the original game's reader overlay: portrait at (15,13),
/// deck icon at (83,13) (58x84 / 68x84 native art), header + word-wrapped
/// transcript in the text rect (15,105,136x175), scroll column at x=159
/// (pgup y=174, pgdn y=203).
const PORTRAIT_POS: (f32, f32) = (15.0, 13.0);
const PORTRAIT_SIZE: (f32, f32) = (58.0, 84.0);
const ICON_POS: (f32, f32) = (83.0, 13.0);
const ICON_SIZE: (f32, f32) = (68.0, 84.0);
const TEXT_X: f32 = 15.0;
const TEXT_W: f32 = 136.0;
/// The header (sender/date) draws at the top of the text rect...
const HEADER_TOP: f32 = 105.0;
const HEADER_LINES: usize = 2;
const LINE_H: f32 = 11.0;
/// ...and the transcript flows below it: (175 - 2*11) / 11 = 13 lines/page.
const BODY_TOP: f32 = HEADER_TOP + HEADER_LINES as f32 * LINE_H;
const PAGE_LINES: usize = 13;
const SCROLL_X: f32 = 159.0;
const PGUP_Y: f32 = 174.0;
const PGDN_Y: f32 = 203.0;
/// Approx characters per line at the 136px rect width (mainfont is
/// variable-width; this is a conservative greedy-wrap budget).
const BODY_WRAP: usize = 26;
const NAME_WRAP: usize = 26;

/// `PropLog` bitmask fields decode as `trailing_zeros + 1`, so a zero (unset)
/// mask reads as 33 - the "no entry" sentinel (research gap #3).
const LOG_UNSET: u32 = 33;

pub struct MediaGui;

#[derive(Clone, Debug, Default)]
pub struct MediaGuiState {
    /// First transcript line shown (paged by the scroll buttons).
    scroll: usize,
}

#[derive(Clone)]
pub enum MediaGuiMsg {
    PageUp,
    PageDown,
}

/// Greedy word-wrap that honors explicit `\n` paragraph breaks. A single word
/// longer than `max_chars` stays on its own (over-long) line rather than being
/// split mid-token, so codes like "45100" are never broken.
fn wrap_text(text: &str, max_chars: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.trim().is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            if current.is_empty() {
                current = word.to_string();
            } else if current.len() + 1 + word.len() <= max_chars {
                current.push(' ');
                current.push_str(word);
            } else {
                lines.push(std::mem::take(&mut current));
                current = word.to_string();
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
    }
    lines
}

fn transcript_line_count(world: &World, entity_id: EntityId) -> usize {
    let v = world.borrow::<View<RuntimePropLogData>>().unwrap();
    v.get(entity_id)
        .ok()
        .and_then(|d| d.text.as_ref().map(|t| wrap_text(t, BODY_WRAP).len()))
        .unwrap_or(0)
}

impl Gui<MediaGuiState, MediaGuiMsg> for MediaGui {
    fn get_components(
        &self,
        _cursor: &Option<GuiCursor>,
        entity_id: EntityId,
        world: &World,
        state: &MediaGuiState,
    ) -> Vec<GuiComponent<MediaGuiMsg>> {
        let mut components: Vec<GuiComponent<MediaGuiMsg>> = vec![
            // Archive-qualified: obj.crf also ships a 64x64 model texture named
            // LOG.PCX (the floppy disc art) and its mount wins the plain name -
            // "iface/" pins the 188x296 MFD frame from the interface archive.
            gui::image("iface/log.pcx")
                .with_position(vec2(0.0, 0.0))
                .with_size(vec2(PANEL_W, PANEL_H)),
        ];

        let v_data = world.borrow::<View<RuntimePropLogData>>().unwrap();
        if let Ok(data) = v_data.get(entity_id) {
            if let Some(portrait) = &data.portrait {
                components.push(
                    gui::image(&format!("{}.pcx", portrait.to_ascii_lowercase()))
                        .with_position(vec2(PORTRAIT_POS.0, PORTRAIT_POS.1))
                        .with_size(vec2(PORTRAIT_SIZE.0, PORTRAIT_SIZE.1)),
                );
            }
            if let Some(icon) = &data.icon {
                components.push(
                    gui::image(&format!("{}.pcx", icon.to_ascii_lowercase()))
                        .with_position(vec2(ICON_POS.0, ICON_POS.1))
                        .with_size(vec2(ICON_SIZE.0, ICON_SIZE.1)),
                );
            }
            if let Some(name) = &data.name {
                for (idx, line) in wrap_text(name, NAME_WRAP)
                    .iter()
                    .take(HEADER_LINES)
                    .enumerate()
                {
                    // Blank lines keep their slot for spacing but must not
                    // become components - an empty string panics the glyph
                    // mesh builder (`SceneObject::screen_space_text`).
                    if line.is_empty() {
                        continue;
                    }
                    components.push(
                        gui::text(line)
                            .with_position(vec2(TEXT_X, HEADER_TOP + idx as f32 * LINE_H))
                            .with_size(vec2(TEXT_W, LINE_H)),
                    );
                }
            }
            if let Some(text) = &data.text {
                let lines = wrap_text(text, BODY_WRAP);
                let start = state.scroll.min(lines.len());
                for (idx, line) in lines[start..].iter().take(PAGE_LINES).enumerate() {
                    if line.is_empty() {
                        continue;
                    }
                    components.push(
                        gui::text(line)
                            .with_position(vec2(TEXT_X, BODY_TOP + idx as f32 * LINE_H))
                            .with_size(vec2(TEXT_W, LINE_H)),
                    );
                }
            }
        }

        // Scroll column (the original's PGUP/PGDN gadgets at x=159).
        components.push(
            gui::button(MediaGuiMsg::PageUp)
                .with_image("pgup0.pcx")
                .with_position(vec2(SCROLL_X, PGUP_Y))
                .with_size(vec2(18.0, 26.0)),
        );
        components.push(
            gui::button(MediaGuiMsg::PageDown)
                .with_image("pgdn0.pcx")
                .with_position(vec2(SCROLL_X, PGDN_Y))
                .with_size(vec2(18.0, 26.0)),
        );

        components
    }

    fn get_config(&self) -> GuiConfig {
        GuiConfig {
            world_offset: Vector3::new(0.0, 0.0, -0.1),
            screen_size_in_pixels: Vector2::new(PANEL_W, PANEL_H),
        }
    }

    fn handle_msg(
        &self,
        entity_id: EntityId,
        world: &World,
        state: &MediaGuiState,
        msg: &MediaGuiMsg,
    ) -> (MediaGuiState, Effect) {
        // Clamp to the last full page: a transcript that fits on one page never
        // scrolls, and PageDown never lands on a near-empty tail page.
        let max_scroll = transcript_line_count(world, entity_id).saturating_sub(PAGE_LINES);
        let scroll = match msg {
            MediaGuiMsg::PageUp => state.scroll.saturating_sub(PAGE_LINES),
            MediaGuiMsg::PageDown => (state.scroll + PAGE_LINES).min(max_scroll),
        };
        (MediaGuiState { scroll }, Effect::NoEffect)
    }

    fn on_frob(&self, entity_id: EntityId, world: &World) -> Effect {
        // Log discs carry `PropLog {deck, email:33, log:N}` - the reader keys
        // off the `log` field; 33 in either field means "not set".
        let (deck, log) = {
            let v_log = world.borrow::<View<PropLog>>().unwrap();
            match v_log.get(entity_id) {
                Ok(log) if log.deck > 0 && log.log > 0 && log.log != LOG_UNSET => {
                    (log.deck, log.log)
                }
                _ => return Effect::NoEffect,
            }
        };
        let audio = Effect::PlaySound {
            handle: AudioHandle::new(),
            name: format!("LOG{deck:02}{log:02}"),
        };
        let collect = Effect::CollectLog {
            entity_id,
            deck,
            log,
        };
        // The original destroyed the disc after its first frob, so its
        // SwitchLinks fired exactly once. The disc now survives for re-reading -
        // keep that one-shot contract by firing the links only on the frob that
        // first collects the log (replaying audio / reopening is fine).
        let already_collected = world
            .borrow::<shipyard::UniqueView<crate::quest_info::QuestInfo>>()
            .map(|q| q.has_collected_log(deck, log))
            .unwrap_or(false);
        let switchlinks = if already_collected {
            Effect::NoEffect
        } else {
            send_to_all_switch_links(world, entity_id, MessagePayload::TurnOn { from: entity_id })
        };
        Effect::combine(vec![collect, audio, switchlinks])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_keeps_codes_intact_and_honors_newlines() {
        let text = "I'll set the new code to 45100.  That should be easy.\nSecond paragraph.";
        let lines = wrap_text(text, 20);
        // The code token is never split across lines.
        assert!(lines.iter().any(|l| l.contains("45100")));
        // The explicit newline forces the second paragraph onto its own line(s).
        assert!(lines.iter().any(|l| l.contains("Second")));
        // No line exceeds the budget except an unsplittable single word.
        for l in &lines {
            assert!(l.split_whitespace().count() <= 1 || l.len() <= 20);
        }
    }

    /// Scroll clamps to the last full page: a transcript that fits on one page
    /// never scrolls, and a longer one stops at `lines - PAGE_LINES` instead of
    /// a near-empty tail page (xreview [AGREED] finding).
    #[test]
    fn page_down_clamps_to_the_last_full_page() {
        let scroll_after = |line_count: usize, presses: usize| {
            let mut world = World::new();
            // `line_count` one-word lines (each word fits one wrapped line).
            let text = vec!["line"; line_count].join("\n");
            let disc = world.add_entity(RuntimePropLogData {
                name: None,
                text: Some(text),
                portrait: None,
                icon: None,
            });
            let gui = MediaGui;
            let mut state = MediaGuiState::default();
            for _ in 0..presses {
                state = gui
                    .handle_msg(disc, &world, &state, &MediaGuiMsg::PageDown)
                    .0;
            }
            state.scroll
        };
        // Fits on one page (5 < 13): PageDown must not move.
        assert_eq!(scroll_after(5, 3), 0);
        // Exactly one page: no scroll either.
        assert_eq!(scroll_after(PAGE_LINES, 2), 0);
        // 20 lines: the only other page starts at 20 - 13 = 7, and stays there.
        assert_eq!(scroll_after(20, 1), 7);
        assert_eq!(scroll_after(20, 5), 7);
        // 27 lines: full second page at 13, then clamp at 27 - 13 = 14.
        assert_eq!(scroll_after(27, 1), 13);
        assert_eq!(scroll_after(27, 2), 14);
    }

    /// Discs whose `log` field is the 33 "not set" sentinel (an empty bitmask -
    /// e.g. an email trap's PropLog) must not collect/play as "log 33".
    #[test]
    fn unset_log_sentinel_is_not_a_log() {
        let mut world = World::new();
        let disc = world.add_entity(PropLog {
            deck: 2,
            email: 1,
            log: LOG_UNSET,
            note: 0,
            video: 0,
        });
        let gui = MediaGui;
        assert!(matches!(gui.on_frob(disc, &world), Effect::NoEffect));
    }
}
