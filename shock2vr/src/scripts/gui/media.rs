//! Audio-log / email reader MFD (`projects/flat-ui-panels.md` §1).
//!
//! The flat-mode reader panel the original opens as `kOverlayEmail`: a `LOG.PCX`
//! backdrop with the sender portrait, deck icon, header line and a word-wrapped,
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

/// Transcript layout on the panel (`shkemail.cpp` text rect ~ (15,105,136,175)).
const BODY_TOP: f32 = 128.0;
const LINE_H: f32 = 11.0;
/// Lines visible per page in the ~152px transcript window below the header.
const PAGE_LINES: usize = 13;
/// Approx characters per transcript line at the 136px rect width (mainfont is
/// variable-width; this is a conservative greedy-wrap budget).
const BODY_WRAP: usize = 30;
const NAME_WRAP: usize = 26;

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
            gui::image("log.pcx")
                .with_position(vec2(0.0, 0.0))
                .with_size(vec2(PANEL_W, PANEL_H)),
        ];

        let v_data = world.borrow::<View<RuntimePropLogData>>().unwrap();
        if let Ok(data) = v_data.get(entity_id) {
            if let Some(portrait) = &data.portrait {
                components.push(
                    gui::image(&format!("{}.pcx", portrait.to_ascii_lowercase()))
                        .with_position(vec2(15.0, 13.0))
                        .with_size(vec2(58.0, 84.0)),
                );
            }
            if let Some(icon) = &data.icon {
                components.push(
                    gui::image(&format!("{}.pcx", icon.to_ascii_lowercase()))
                        .with_position(vec2(120.0, 13.0))
                        .with_size(vec2(40.0, 40.0)),
                );
            }
            if let Some(name) = &data.name {
                for (idx, line) in wrap_text(name, NAME_WRAP).iter().take(2).enumerate() {
                    components.push(
                        gui::text(line)
                            .with_position(vec2(15.0, 100.0 + idx as f32 * LINE_H))
                            .with_size(vec2(158.0, LINE_H)),
                    );
                }
            }
            if let Some(text) = &data.text {
                let lines = wrap_text(text, BODY_WRAP);
                let start = state.scroll.min(lines.len());
                for (idx, line) in lines[start..].iter().take(PAGE_LINES).enumerate() {
                    components.push(
                        gui::text(line)
                            .with_position(vec2(15.0, BODY_TOP + idx as f32 * LINE_H))
                            .with_size(vec2(140.0, LINE_H)),
                    );
                }
            }
        }

        // Scroll column (the original's PGUP/PGDN gadgets at x=159).
        components.push(
            gui::button(MediaGuiMsg::PageUp)
                .with_image("pgup0.pcx")
                .with_position(vec2(159.0, 190.0))
                .with_size(vec2(18.0, 26.0)),
        );
        components.push(
            gui::button(MediaGuiMsg::PageDown)
                .with_image("pgdn0.pcx")
                .with_position(vec2(159.0, 222.0))
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
        let max_scroll = transcript_line_count(world, entity_id).saturating_sub(1);
        let scroll = match msg {
            MediaGuiMsg::PageUp => state.scroll.saturating_sub(PAGE_LINES),
            MediaGuiMsg::PageDown => (state.scroll + PAGE_LINES).min(max_scroll),
        };
        (MediaGuiState { scroll }, Effect::NoEffect)
    }

    fn on_frob(&self, entity_id: EntityId, world: &World) -> Effect {
        // Log discs carry `PropLog {deck, email:33, log:N}` - the reader keys off
        // the `log` field (33-in-`email` is a "not set" sentinel, research gap #3).
        let (deck, log) = {
            let v_log = world.borrow::<View<PropLog>>().unwrap();
            match v_log.get(entity_id) {
                Ok(log) if log.deck > 0 && log.log > 0 => (log.deck, log.log),
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
        let switchlinks =
            send_to_all_switch_links(world, entity_id, MessagePayload::TurnOn { from: entity_id });
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
}
