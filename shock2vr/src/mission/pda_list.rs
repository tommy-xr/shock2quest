//! PDA list: every collected email and audio log, per deck, as the retail
//! LOGS MFD shows them, plus the deck's objective notes as a read-only
//! checklist. Selecting an email or log opens the shared media reader,
//! whose return button comes back here. Rects are the retail PDA overlay's,
//! relative to the left MFD panel, and resolved once on the shared canvas.
use crate::{
    game_scene::DebugUiElement,
    quest_info::{PdaEntry, PdaKind, QuestInfo},
    ui::{HAlign, MFD_FONT, Rect, UiCanvas, VAlign},
};
use cgmath::Vector2;
use engine::assets::asset_cache::AssetCache;
use shipyard::{UniqueView, World};
use std::collections::HashMap;

const PANEL: Rect = Rect::new(2.0, 124.0, 188.0, 296.0);
const VISIBLE_ROWS: usize = 9;
const ROW_TOP: f32 = 36.0;
const ROW_DY: f32 = 24.0;
const LIST_X: f32 = 22.0;
const LIST_W: f32 = 130.0;
const LINE_H: f32 = 12.0;
/// Notes page 6 rows of up to 3 wrapped lines; the 16x24 checkbox sits 12 px
/// left of the list and the text 2 px past it.
const NOTE_ROWS: usize = 6;
const NOTE_DY: f32 = 36.0;
const NOTE_LINES: usize = 3;
const NOTE_BOX: Rect = Rect::new(LIST_X - 12.0, 0.0, 16.0, 24.0);
const NOTE_TEXT_X: f32 = NOTE_BOX.x + NOTE_BOX.w + 2.0;
const NOTE_TEXT_W: f32 = LIST_W - 5.0;

/// The tabs along the bottom: kind, art stem, x, label.
const TABS: [(PdaKind, &str, f32, &str); 3] = [
    (PdaKind::Email, "pemail", 12.0, "pda_tab_email"),
    (PdaKind::Log, "plogs", 53.0, "pda_tab_logs"),
    (PdaKind::Note, "pnotes", 94.0, "pda_tab_notes"),
];

/// What a pointer press on the list asks its owner to do.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum PdaAction {
    None,
    Close,
    Open(PdaEntry),
}

#[derive(Debug, PartialEq)]
struct NoteRow {
    checked: bool,
    lines: Vec<String>,
}

#[derive(Default)]
pub(super) struct PdaList {
    kind: PdaKind,
    deck: u32,
    /// First visible row; the arrows scroll one entry at a time like retail.
    offset: usize,
    decks: Vec<u32>,
    rows: Vec<(u32, String)>,
    notes: Vec<NoteRow>,
    header: String,
    deck_name: String,
}

impl PdaList {
    /// Show `kind` on `deck`, scrolled so `focus` (the entry the reader is
    /// returning from) stays visible.
    pub(super) fn open(kind: PdaKind, deck: u32, focus: Option<u32>, quests: &QuestInfo) -> Self {
        let position = focus.and_then(|index| {
            quests
                .pda_entries(kind, deck)
                .iter()
                .position(|entry| *entry == index)
        });
        Self {
            kind,
            deck,
            offset: position.map_or(0, |p| (p + 1).saturating_sub(VISIBLE_ROWS)),
            ..Default::default()
        }
    }

    pub(super) fn refresh(&mut self, world: &World, assets: &mut AssetCache) {
        let Ok(quests) = world.borrow::<UniqueView<QuestInfo>>() else {
            return;
        };
        self.decks = quests.pda_decks();
        let entries = quests.pda_entries(self.kind, self.deck);
        let notes = quests.pda_notes(self.deck);
        drop(quests);
        let misc = assets.get(&dark::importers::STRINGS_IMPORTER, "misc.str");
        let (base, prefix, fallback) = match self.kind {
            PdaKind::Email => ("levelbase0", "emailname", "Emails: Deck %d"),
            PdaKind::Log => ("levelbase1", "logname", "Logs: Deck %d"),
            PdaKind::Note => ("levelbase2", "", "Notes: Deck %d"),
        };
        self.header = misc
            .get(base)
            .map_or(fallback, String::as_str)
            .replace("%d", &self.deck.to_string());
        self.deck_name = misc
            .get(&format!("level{}", self.deck.saturating_sub(1)))
            .cloned()
            .unwrap_or_default();
        if self.kind == PdaKind::Note {
            let strings = assets.get_opt(&dark::importers::STRINGS_IMPORTER, "notes.str");
            self.rows.clear();
            self.notes = note_rows(self.deck, notes, strings.as_deref(), |text| {
                crate::scripts::gui::PanelText::wrap(world, text, NOTE_TEXT_W)
            });
            self.offset = self.offset.min(self.max_offset());
            return;
        }
        self.notes.clear();
        let level = assets.get_opt(
            &dark::importers::STRINGS_IMPORTER,
            &format!("level{:02}.str", self.deck),
        );
        self.rows = entries
            .into_iter()
            .map(|index| {
                let name = level
                    .as_ref()
                    .and_then(|strings| strings.get(&format!("{prefix}{index}")))
                    .map(|name| name.replace("\\n", "\n"))
                    .unwrap_or_else(|| format!("#{index}"));
                (index, name)
            })
            .collect();
        self.offset = self.offset.min(self.max_offset());
    }

    fn len(&self) -> usize {
        if self.kind == PdaKind::Note {
            self.notes.len()
        } else {
            self.rows.len()
        }
    }

    fn page_rows(&self) -> usize {
        if self.kind == PdaKind::Note {
            NOTE_ROWS
        } else {
            VISIBLE_ROWS
        }
    }

    fn max_offset(&self) -> usize {
        self.len().saturating_sub(self.page_rows())
    }

    /// Retail's deck arrows wrap through the decks that hold anything.
    fn step_deck(&mut self, forward: bool) {
        let mut decks = self.decks.clone();
        if !decks.contains(&self.deck) {
            decks.push(self.deck);
            decks.sort_unstable();
        }
        let at = decks.iter().position(|d| *d == self.deck).unwrap_or(0);
        let next = if forward {
            (at + 1) % decks.len()
        } else {
            (at + decks.len() - 1) % decks.len()
        };
        self.deck = decks[next];
        self.offset = 0;
    }

    pub(super) fn update(&mut self, point: Vector2<f32>, pressed: bool) -> PdaAction {
        if !pressed {
            return PdaAction::None;
        }
        let elements = self.elements();
        let Some(label) = elements
            .iter()
            .find(|e| {
                e.kind == "button"
                    && Rect::new(e.rect[0], e.rect[1], e.rect[2], e.rect[3]).contains(point)
            })
            .and_then(|e| e.label.clone())
        else {
            return PdaAction::None;
        };
        if let Some((kind, ..)) = TABS.iter().find(|tab| tab.3 == label) {
            self.kind = *kind;
            self.offset = 0;
            return PdaAction::None;
        }
        match label.as_str() {
            "utility_close" => return PdaAction::Close,
            "pda_deck_previous" => self.step_deck(false),
            "pda_deck_next" => self.step_deck(true),
            "pda_scroll_up" => self.offset = self.offset.saturating_sub(1),
            "pda_scroll_down" => self.offset = (self.offset + 1).min(self.max_offset()),
            _ => {
                if let Some((index, _)) = label
                    .strip_prefix("pda_entry:")
                    .and_then(|n| n.parse::<usize>().ok())
                    .and_then(|row| self.rows.get(row))
                {
                    return PdaAction::Open(PdaEntry {
                        kind: self.kind,
                        deck: self.deck,
                        index: *index,
                    });
                }
            }
        }
        PdaAction::None
    }

    pub(super) fn contains(&self, point: Vector2<f32>) -> bool {
        PANEL.contains(point)
    }

    pub(super) fn elements(&self) -> Vec<DebugUiElement> {
        let mut out = Vec::new();
        let mut add =
            |kind: &str, rect: Rect, texture: Option<&str>, text: Option<&str>, label: String| {
                let r = [PANEL.x + rect.x, PANEL.y + rect.y, rect.w, rect.h];
                out.push(DebugUiElement {
                    kind: kind.into(),
                    texture: texture.map(str::to_owned),
                    text: text.map(str::to_owned),
                    label: Some(label),
                    entity_id: None,
                    rect: r,
                    screen_rect: r,
                });
            };
        add(
            "image",
            Rect::new(0.0, 0.0, 188.0, 296.0),
            Some("iface/pda.pcx"),
            None,
            "pda".into(),
        );
        add(
            "button",
            Rect::new(163.0, 8.0, 20.0, 21.0),
            Some("iface/closeoff.pcx"),
            None,
            "utility_close".into(),
        );
        add(
            "button",
            Rect::new(13.0, 11.0, 18.0, 18.0),
            Some("iface/left0.pcx"),
            None,
            "pda_deck_previous".into(),
        );
        add(
            "button",
            Rect::new(137.0, 11.0, 18.0, 18.0),
            Some("iface/right0.pcx"),
            None,
            "pda_deck_next".into(),
        );
        add(
            "text",
            Rect::new(35.0, 13.0, 95.0, 10.0),
            None,
            Some(&self.header),
            "pda_header".into(),
        );
        add(
            "text",
            Rect::new(35.0, 23.0, 95.0, 10.0),
            None,
            Some(&self.deck_name),
            "pda_header".into(),
        );
        if self.len() == 0 {
            let empty = match self.kind {
                PdaKind::Email => "No collected emails.",
                PdaKind::Log => "No collected logs.",
                PdaKind::Note => "No notes.",
            };
            add(
                "text",
                Rect::new(LIST_X, ROW_TOP, LIST_W, LINE_H),
                None,
                Some(empty),
                "utility_text".into(),
            );
        }
        for (row, (_, name)) in self
            .rows
            .iter()
            .enumerate()
            .skip(self.offset)
            .take(VISIBLE_ROWS)
        {
            let y = ROW_TOP + (row - self.offset) as f32 * ROW_DY;
            add(
                "button",
                Rect::new(13.0, y, 139.0, ROW_DY),
                None,
                Some(name),
                format!("pda_entry:{row}"),
            );
            // Names are two authored lines: sender + date, then subject.
            for (line, text) in name
                .lines()
                .filter(|l| !l.trim().is_empty())
                .take(2)
                .enumerate()
            {
                let rect = Rect::new(LIST_X, y + line as f32 * LINE_H, LIST_W, LINE_H);
                add("text", rect, None, Some(text.trim()), "utility_text".into());
            }
        }
        // Notes are not buttons: the checklist is read-only.
        for (row, note) in self
            .notes
            .iter()
            .enumerate()
            .skip(self.offset)
            .take(NOTE_ROWS)
        {
            let y = ROW_TOP + (row - self.offset) as f32 * NOTE_DY;
            let checkbox = Rect { y, ..NOTE_BOX };
            add(
                "image",
                checkbox,
                Some("iface/notebox.pcx"),
                None,
                format!("pda_note:{row}"),
            );
            if note.checked {
                add(
                    "image",
                    checkbox,
                    Some("iface/notechek.pcx"),
                    None,
                    format!("pda_note_checked:{row}"),
                );
            }
            for (line, text) in note.lines.iter().enumerate() {
                let rect = Rect::new(NOTE_TEXT_X, y + line as f32 * LINE_H, NOTE_TEXT_W, LINE_H);
                add(
                    "text",
                    rect,
                    None,
                    Some(text),
                    format!("pda_note_text:{row}"),
                );
            }
        }
        add(
            "button",
            Rect::new(157.0, 152.0, 18.0, 17.0),
            Some("iface/up0.pcx"),
            None,
            "pda_scroll_up".into(),
        );
        add(
            "button",
            Rect::new(157.0, 247.0, 18.0, 17.0),
            Some("iface/down0.pcx"),
            None,
            "pda_scroll_down".into(),
        );
        for (kind, art, x, label) in TABS {
            // The `1` art is the pressed state; retail draws it on the active tab.
            let state = if kind == self.kind { 1 } else { 0 };
            let texture = format!("iface/{art}{state}.pcx");
            add(
                "button",
                Rect::new(x, 268.0, 40.0, 18.0),
                Some(&texture),
                None,
                label.into(),
            );
        }
        out
    }

    pub(super) fn draw(&self, canvas: &mut UiCanvas) {
        for e in self.elements() {
            let r = Rect::new(e.rect[0], e.rect[1], e.rect[2], e.rect[3]);
            if let Some(texture) = &e.texture {
                canvas.image(r, texture);
            }
            if let Some(text) = e
                .text
                .as_deref()
                .filter(|t| e.kind == "text" && !t.is_empty())
            {
                let align = if e.label.as_deref() == Some("pda_header") {
                    HAlign::Center
                } else {
                    HAlign::Left
                };
                canvas.text_native_fit(r, text, MFD_FONT, align, VAlign::Top);
            }
        }
    }
}

/// A deck's notes in retail's order - notes.str `NoteOrder_<deck>_<n>`
/// ascending, a missing order first - each wrapped to `NOTE_LINES` lines.
fn note_rows(
    deck: u32,
    mut notes: Vec<(u32, bool)>,
    strings: Option<&HashMap<String, String>>,
    wrap: impl Fn(&str) -> Vec<String>,
) -> Vec<NoteRow> {
    let lookup = |key: String| strings.and_then(|strings| strings.get(&key));
    notes.sort_by_key(|(n, _)| {
        lookup(format!("noteorder_{deck}_{n}"))
            .and_then(|order| order.trim().parse::<i32>().ok())
            .unwrap_or(0)
    });
    notes
        .into_iter()
        .map(|(n, checked)| {
            let text = lookup(format!("note_{deck}_{n}"))
                .map_or_else(|| format!("#{n}"), |text| text.replace("\\n", "\n"));
            let mut lines = wrap(text.trim());
            lines.truncate(NOTE_LINES);
            NoteRow { checked, lines }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use dark::properties::QuestBitValue;

    fn quests(logs: &[(u32, u32)]) -> QuestInfo {
        let mut quests = QuestInfo::new();
        for (deck, log) in logs {
            quests.collect_log(*deck, *log);
        }
        quests
    }

    fn list(kind: PdaKind, deck: u32, quests: &QuestInfo) -> PdaList {
        let mut list = PdaList::open(kind, deck, None, quests);
        fill(&mut list, quests);
        list
    }

    /// `refresh` without the asset cache: one row set for the current tab.
    fn fill(list: &mut PdaList, quests: &QuestInfo) {
        list.decks = quests.pda_decks();
        list.rows.clear();
        list.notes.clear();
        if list.kind == PdaKind::Note {
            let notes = quests.pda_notes(list.deck);
            list.notes = note_rows(list.deck, notes, None, |text| vec![text.to_owned()]);
        } else {
            list.rows = quests
                .pda_entries(list.kind, list.deck)
                .into_iter()
                .map(|index| (index, format!("SENDER\nre: entry {index}\n")))
                .collect();
        }
    }

    fn notes(bits: &[(&str, QuestBitValue)]) -> QuestInfo {
        let mut quests = QuestInfo::new();
        for (name, value) in bits {
            quests.set_quest_bit_value(name, *value);
        }
        quests
    }

    fn textures(list: &PdaList) -> Vec<String> {
        list.elements()
            .into_iter()
            .filter_map(|e| e.texture)
            .collect()
    }

    fn press(list: &mut PdaList, label: &str) -> PdaAction {
        let element = list
            .elements()
            .into_iter()
            .find(|e| e.kind == "button" && e.label.as_deref() == Some(label))
            .unwrap_or_else(|| panic!("{label} is drawn"));
        let point = Vector2::new(element.rect[0] + 2.0, element.rect[1] + 2.0);
        list.update(point, true)
    }

    /// The older log is selectable, not just the newest one.
    #[test]
    fn every_collected_log_on_the_deck_opens_from_its_row() {
        let quests = quests(&[(2, 20), (2, 4)]);
        let mut list = list(PdaKind::Log, 2, &quests);

        assert_eq!(
            press(&mut list, "pda_entry:0"),
            PdaAction::Open(PdaEntry {
                kind: PdaKind::Log,
                deck: 2,
                index: 20
            })
        );
        assert_eq!(
            press(&mut list, "pda_entry:1"),
            PdaAction::Open(PdaEntry {
                kind: PdaKind::Log,
                deck: 2,
                index: 4
            })
        );
        assert_eq!(press(&mut list, "utility_close"), PdaAction::Close);
    }

    #[test]
    fn tabs_and_deck_arrows_switch_the_list_and_wrap() {
        let quests = quests(&[(2, 1), (4, 1)]);
        let mut list = list(PdaKind::Log, 2, &quests);

        press(&mut list, "pda_deck_next");
        assert_eq!(list.deck, 4);
        press(&mut list, "pda_deck_next");
        assert_eq!(list.deck, 2, "wraps to the first deck");
        press(&mut list, "pda_deck_previous");
        assert_eq!(list.deck, 4);
        press(&mut list, "pda_tab_email");
        assert_eq!(list.kind, PdaKind::Email);
        assert!(
            list.elements()
                .iter()
                .any(|e| e.texture.as_deref() == Some("iface/pemail1.pcx")),
            "active tab draws its pressed art"
        );
    }

    /// NoteOrder sorts ascending with a missing order first (retail's 0), and
    /// each note keeps at most three wrapped lines.
    #[test]
    fn notes_sort_by_note_order_and_wrap_to_three_lines() {
        let strings: HashMap<String, String> = [
            ("note_2_4", "The code for Cryo A is 45100."),
            ("noteorder_2_4", "203"),
            ("note_2_2", "Get through a secure airlock."),
            ("noteorder_2_2", "202"),
            ("note_2_9", "one two three four five"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
        let words = |text: &str| text.split(' ').map(str::to_owned).collect();

        let rows = note_rows(
            2,
            vec![(2, false), (4, true), (9, false)],
            Some(&strings),
            words,
        );

        assert_eq!(
            rows,
            vec![
                NoteRow {
                    checked: false,
                    lines: vec!["one".into(), "two".into(), "three".into()],
                },
                NoteRow {
                    checked: false,
                    lines: words("Get through a secure airlock.")
                        .into_iter()
                        .take(3)
                        .collect(),
                },
                NoteRow {
                    checked: true,
                    lines: words("The code for Cryo A is 45100.")
                        .into_iter()
                        .take(3)
                        .collect(),
                },
            ]
        );
    }

    /// The notes tab draws its pressed art, a box per visible note with the
    /// check over completed ones, and no clickable rows.
    #[test]
    fn notes_tab_shows_a_read_only_checklist() {
        let quests = notes(&[
            ("Note_2_6", QuestBitValue::INCOMPLETE),
            ("Note_2_9", QuestBitValue::COMPLETE),
        ]);
        let mut list = list(PdaKind::Log, 2, &quests);
        assert!(
            list.elements()
                .iter()
                .any(|e| e.text.as_deref() == Some("No collected logs."))
        );

        press(&mut list, "pda_tab_notes");
        assert_eq!(list.kind, PdaKind::Note);
        fill(&mut list, &quests);
        let textures = textures(&list);
        assert!(textures.contains(&"iface/pnotes1.pcx".to_owned()));
        assert!(textures.contains(&"iface/plogs0.pcx".to_owned()));
        assert_eq!(
            textures
                .iter()
                .filter(|t| *t == "iface/notebox.pcx")
                .count(),
            2
        );
        assert_eq!(
            textures
                .iter()
                .filter(|t| *t == "iface/notechek.pcx")
                .count(),
            1
        );
        let elements = list.elements();
        let text = |row: usize| {
            elements
                .iter()
                .find(|e| e.label.as_deref() == Some(&format!("pda_note_text:{row}")))
                .and_then(|e| e.text.clone())
        };
        assert_eq!(text(0).as_deref(), Some("#6"));
        assert_eq!(text(1).as_deref(), Some("#9"));
        assert!(!elements.iter().any(|e| {
            e.label
                .as_deref()
                .is_some_and(|l| l.starts_with("pda_entry:"))
        }));
        let note = elements
            .iter()
            .find(|e| e.label.as_deref() == Some("pda_note:0"))
            .unwrap();
        let checkbox_to_text = text_rect(&elements, "pda_note_text:0")[0] - note.rect[0];
        assert_eq!(
            checkbox_to_text, 18.0,
            "text starts 2 px past the 16 px box"
        );
        assert_eq!(
            list.update(Vector2::new(note.rect[0] + 2.0, note.rect[1] + 2.0), true),
            PdaAction::None
        );

        let empty = notes(&[("Note_2_6", QuestBitValue::UNKNOWN)]);
        let mut list = self::list(PdaKind::Note, 2, &empty);
        fill(&mut list, &empty);
        assert!(
            list.elements()
                .iter()
                .any(|e| e.text.as_deref() == Some("No notes."))
        );
    }

    fn text_rect(elements: &[DebugUiElement], label: &str) -> [f32; 4] {
        elements
            .iter()
            .find(|e| e.label.as_deref() == Some(label))
            .unwrap_or_else(|| panic!("{label} is drawn"))
            .rect
    }

    /// A deck holding only notes is still a deck-arrow stop, and notes page
    /// a six-row window.
    #[test]
    fn note_only_decks_cycle_and_notes_scroll_six_rows() {
        let mut quests = QuestInfo::new();
        for n in 1..=8 {
            quests.set_quest_bit_value(&format!("Note_5_{n}"), QuestBitValue::INCOMPLETE);
        }
        quests.collect_log(2, 1);
        let mut list = list(PdaKind::Note, 2, &quests);

        press(&mut list, "pda_deck_next");
        assert_eq!(list.deck, 5);
        fill(&mut list, &quests);
        assert_eq!(
            textures(&list)
                .iter()
                .filter(|t| *t == "iface/notebox.pcx")
                .count(),
            6
        );
        for _ in 0..5 {
            press(&mut list, "pda_scroll_down");
        }
        assert_eq!(list.offset, 2, "stops on the last full page");
        press(&mut list, "pda_deck_next");
        assert_eq!(list.deck, 2, "wraps back past the note-only deck");
    }

    /// Returning from an entry past the first page keeps it on screen.
    #[test]
    fn returning_focus_scrolls_the_entry_into_view_and_scrolling_clamps() {
        let logs: Vec<(u32, u32)> = (1..=12).map(|log| (3, log)).collect();
        let quests = quests(&logs);
        let list = PdaList::open(PdaKind::Log, 3, Some(12), &quests);
        assert_eq!(list.offset, 3);

        let mut list = self::list(PdaKind::Log, 3, &quests);
        for _ in 0..10 {
            press(&mut list, "pda_scroll_down");
        }
        assert_eq!(list.offset, 3, "never scrolls past the last full page");
        for e in list.elements() {
            assert!(e.rect[0] >= PANEL.x && e.rect[1] >= PANEL.y);
            assert!(e.rect[0] + e.rect[2] <= PANEL.x + PANEL.w);
            assert!(e.rect[1] + e.rect[3] <= PANEL.y + PANEL.h);
        }
    }
}
