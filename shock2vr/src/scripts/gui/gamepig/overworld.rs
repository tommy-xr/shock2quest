use super::GamePigGuiMsg;
use crate::{
    gui::{self, GuiComponent},
    handheld_input::HandheldControl,
    overworld::{Action, Message as Notice, Phase},
    overworld_game::Command,
    quest_info::QuestInfo,
    scripts::{Effect, gui::PanelText},
    time::Time,
    ui::Rect,
};
use cgmath::vec2;
use shipyard::{EntityId, UniqueView, World};
use std::time::Duration;

#[derive(Clone, Copy, Debug)]
pub enum Message {
    Tile(i32, i32),
    Wait,
    Potion,
    Cast,
    Look,
    Back,
    Continue,
    Restart,
}
#[derive(Clone, Copy, Debug)]
enum Aim {
    Cast,
    Look,
}
#[derive(Clone, Debug, Default)]
pub(super) struct Adventure {
    menu: bool,
    direction: (i32, i32),
    selection: usize,
    aim: Option<Aim>,
    scores: bool,
    finished_time: Duration,
    last_frame: Option<Duration>,
}

const FIELD: Rect = Rect::new(16.0, 12.0, 132.0, 204.0);
const ACTIONS: [(&str, Message); 5] = [
    ("Wait", Message::Wait),
    ("Heal", Message::Potion),
    ("Cast", Message::Cast),
    ("Look", Message::Look),
    ("Back", Message::Back),
];

fn button(
    message: Message,
    label: &str,
    text: &str,
    rect: Rect,
) -> Vec<GuiComponent<GamePigGuiMsg>> {
    vec![
        gui::button(GamePigGuiMsg::OverWorld(message))
            .with_rect(rect)
            .with_label(label)
            .with_alpha(0.0),
        PanelText::centered(text, rect),
    ]
}
fn effect(entity_id: EntityId, command: Command) -> Effect {
    Effect::UpdateOverWorld { entity_id, command }
}

impl Adventure {
    pub fn advance(&mut self, entity: EntityId, world: &World, time: &Time) -> Effect {
        if self.last_frame == Some(time.total) {
            return Effect::NoEffect;
        }
        self.last_frame = Some(time.total);
        let quest = world.borrow::<UniqueView<QuestInfo>>().unwrap();
        let Some(game) = &quest.overworld.game else {
            return Effect::NoEffect;
        };
        if game.phase != Phase::Playing {
            self.finished_time += time.elapsed;
        } else {
            self.finished_time = Duration::ZERO;
        }
        effect(
            entity,
            Command::Advance {
                frame: time.total,
                elapsed: time.elapsed,
            },
        )
    }
    pub fn input(&mut self, entity: EntityId, world: &World, message: Message) -> Effect {
        let quest = world.borrow::<UniqueView<QuestInfo>>().unwrap();
        let Some(game) = &quest.overworld.game else {
            return Effect::NoEffect;
        };
        if game.phase != Phase::Playing {
            if self.finished_time < Duration::from_secs(3) {
                return Effect::NoEffect;
            }
            match message {
                Message::Continue => self.scores = true,
                Message::Restart if self.scores => {
                    *self = Self::default();
                    return effect(entity, Command::Restart);
                }
                _ => (),
            }
            return Effect::NoEffect;
        }
        self.menu = false;
        let action = match message {
            Message::Tile(x, y) => {
                let (x, y) = (x - 5, y - 8);
                match self.aim.take() {
                    Some(Aim::Cast) => Action::Missile(x.signum(), y.signum()),
                    Some(Aim::Look) => Action::Inspect(game.player.position.offset(x, y)),
                    None => Action::Move(x.signum(), y.signum()),
                }
            }
            Message::Wait => match self.aim.take() {
                Some(Aim::Look) => Action::Inspect(game.player.position),
                _ => Action::Wait,
            },
            Message::Potion => {
                self.aim = None;
                Action::Potion
            }
            Message::Cast => {
                if can_cast(world, entity, &quest) {
                    self.aim = Some(Aim::Cast);
                }
                return Effect::NoEffect;
            }
            Message::Look => {
                self.aim = Some(Aim::Look);
                return Effect::NoEffect;
            }
            _ => return Effect::NoEffect,
        };
        effect(entity, Command::Act(action))
    }
    pub fn controller(
        &mut self,
        entity: EntityId,
        world: &World,
        control: HandheldControl,
    ) -> Effect {
        match control {
            // Navigate supplies the shared press edge; its preceding Stick retains both axes.
            HandheldControl::Stick { x, y } => {
                self.direction = (
                    if x.abs() >= 0.5 { x.signum() as i32 } else { 0 },
                    if y.abs() >= 0.5 {
                        -y.signum() as i32
                    } else {
                        0
                    },
                );
                Effect::NoEffect
            }
            HandheldControl::Navigate { x, y } if self.menu => {
                self.selection = (self.selection as i32 + x + y).rem_euclid(5) as usize;
                let quest = world.borrow::<UniqueView<QuestInfo>>().unwrap();
                if self.selection == 2 && !can_cast(world, entity, &quest) {
                    self.selection =
                        (self.selection as i32 + (x + y).signum()).rem_euclid(5) as usize;
                }
                Effect::NoEffect
            }
            HandheldControl::Navigate { .. } => self.input(
                entity,
                world,
                Message::Tile(5 + self.direction.0, 8 + self.direction.1),
            ),
            HandheldControl::Primary => {
                let quest = world.borrow::<UniqueView<QuestInfo>>().unwrap();
                let finished = quest
                    .overworld
                    .game
                    .as_ref()
                    .is_some_and(|g| g.phase != Phase::Playing);
                drop(quest);
                let message = if finished {
                    if self.scores {
                        Message::Restart
                    } else {
                        Message::Continue
                    }
                } else if self.menu {
                    ACTIONS[self.selection].1
                } else {
                    Message::Wait
                };
                self.input(entity, world, message)
            }
            HandheldControl::Secondary => {
                self.menu = !self.menu;
                self.aim = None;
                Effect::NoEffect
            }
            _ => Effect::NoEffect,
        }
    }
    pub fn wants_back(&self) -> bool {
        self.menu && self.selection == 4
    }
    pub fn inspect(&self, entity: EntityId, world: &World, point: cgmath::Point2<f32>) -> Effect {
        if !FIELD.contains(vec2(point.x, point.y)) {
            return Effect::NoEffect;
        }
        let quest = world.borrow::<UniqueView<QuestInfo>>().unwrap();
        let Some(game) = &quest.overworld.game else {
            return Effect::NoEffect;
        };
        let x = ((point.x - FIELD.x) / 12.0) as i32 - 5;
        let y = ((point.y - FIELD.y) / 12.0) as i32 - 8;
        effect(
            entity,
            Command::Act(Action::Inspect(game.player.position.offset(x, y))),
        )
    }
    pub fn components(&self, world: &World, entity: EntityId) -> Vec<GuiComponent<GamePigGuiMsg>> {
        let quest = world.borrow::<UniqueView<QuestInfo>>().unwrap();
        let Some(game) = &quest.overworld.game else {
            return Vec::new();
        };
        let mut out = vec![GuiComponent::Fill {
            position: vec2(13.0, 11.0),
            size: vec2(140.0, 276.0),
            color: [0, 0, 0],
            alpha: 1.0,
        }];
        if self.scores {
            out.extend(super::score_table(
                world,
                4,
                quest.overworld.score_rank,
                Rect::new(16.0, 25.0, 132.0, 230.0),
            ));
            out.extend(button(
                Message::Restart,
                "ow-restart",
                "New adventure",
                Rect::new(16.0, 245.0, 132.0, 16.0),
            ));
            return out;
        }
        let frame = if self.last_frame.unwrap_or_default().as_millis() / 500 % 2 == 0 {
            "a"
        } else {
            "b"
        };
        for y in 0..17 {
            for x in 0..11 {
                let p = game.player.position.offset(x - 5, y - 8);
                let tile = game.map.tile(p);
                let center = (9 + x * 12, 7 + y * 12);
                out.push(super::sprite(
                    &format!("owterr{:02}.pcx", tile.terrain as u8),
                    center.0,
                    center.1,
                    12.0,
                    12.0,
                ));
                if let Some(loot) = tile.loot {
                    out.push(super::sprite(
                        &format!("owloot{:02}.pcx", loot as u8),
                        center.0,
                        center.1,
                        12.0,
                        12.0,
                    ));
                }
                let actor = if p == game.player.position {
                    Some(0)
                } else {
                    game.actor_at(p).map(|(_, a)| a.kind as u8)
                };
                if let Some(kind) = actor {
                    out.push(super::sprite(
                        &format!("owmon{kind:02}{frame}.pcx"),
                        center.0,
                        center.1,
                        12.0,
                        12.0,
                    ));
                }
                for animation in game.animations.iter().filter(|a| a.position == p) {
                    out.push(super::sprite(
                        &format!("owcmbt{:02}.pcx", animation.visual as u8),
                        center.0,
                        center.1,
                        12.0,
                        12.0,
                    ));
                }
                if game.phase == Phase::Playing {
                    out.push(
                        gui::button(GamePigGuiMsg::OverWorld(Message::Tile(x, y)))
                            .with_rect(Rect::new(
                                FIELD.x + x as f32 * 12.0,
                                FIELD.y + y as f32 * 12.0,
                                12.0,
                                12.0,
                            ))
                            .with_label(&format!("ow-tile-{x}-{y}"))
                            .with_alpha(0.0),
                    );
                }
            }
        }
        let p = &game.player;
        for (key, fallback, args, rect) in [
            (
                "ow_stats_hp_short",
                "Hp:%d/%d",
                vec![p.hp, p.max_hp],
                Rect::new(16.0, 218.0, 75.0, 11.0),
            ),
            (
                "ow_stats_gp",
                "GP: %d",
                vec![p.gold.min(i32::MAX as u32) as i32],
                Rect::new(93.0, 218.0, 55.0, 11.0),
            ),
            (
                "ow_stats_lnorm",
                "Level: %d",
                vec![i32::from(p.level)],
                Rect::new(16.0, 230.0, 46.0, 11.0),
            ),
            (
                "ow_stats_xp",
                "Xp: %d",
                vec![p.xp.min(i32::MAX as u32) as i32],
                Rect::new(93.0, 230.0, 55.0, 11.0),
            ),
        ] {
            out.push(PanelText::text(
                &PanelText::format(&PanelText::string(world, "minigame", key, fallback), &args),
                rect,
            ));
        }
        out.push(super::sprite("owloot07.pcx", 56, 225, 12.0, 12.0));
        out.push(PanelText::text(
            &p.potions.to_string(),
            Rect::new(77.0, 230.0, 15.0, 11.0),
        ));
        let lines: Vec<_> = game
            .messages
            .iter()
            .flat_map(|m| PanelText::wrap(world, &notice(world, m), 132.0))
            .collect();
        for (row, line) in lines
            .iter()
            .rev()
            .take(3)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .enumerate()
        {
            out.push(PanelText::text(
                line,
                Rect::new(16.0, 243.0 + row as f32 * 10.0, 132.0, 10.0),
            ));
        }
        if game.phase != Phase::Playing {
            let (key, fallback) = if game.phase == Phase::Won {
                ("ow_youwon", "You Won")
            } else {
                ("ow_gameover", "Game Over")
            };
            out.push(GuiComponent::Fill {
                position: vec2(25.0, 92.0),
                size: vec2(114.0, 28.0),
                color: [0, 0, 0],
                alpha: 0.9,
            });
            out.push(PanelText::centered(
                &PanelText::string(world, "minigame", key, fallback),
                Rect::new(25.0, 92.0, 114.0, 28.0),
            ));
            if self.finished_time >= Duration::from_secs(3) {
                out.extend(button(
                    Message::Continue,
                    "ow-continue",
                    "Scores",
                    Rect::new(16.0, 274.0, 132.0, 12.0),
                ));
            }
        } else {
            for (i, (title, message)) in ACTIONS.into_iter().enumerate() {
                let rect = Rect::new(16.0 + i as f32 * 26.4, 274.0, 26.4, 12.0);
                if i == 2 && !can_cast(world, entity, &quest) {
                    out.push(PanelText::centered(title, rect).with_alpha(0.35));
                } else {
                    out.extend(button(
                        message,
                        ["ow-wait", "ow-heal", "ow-cast", "ow-look", "ow-back"][i],
                        title,
                        rect,
                    ));
                }
                if self.menu && self.selection == i {
                    super::highlight(&mut out, rect);
                }
            }
            if let Some(aim) = self.aim {
                out.push(GuiComponent::Fill {
                    position: vec2(16.0, 243.0),
                    size: vec2(132.0, 30.0),
                    color: [0, 0, 0],
                    alpha: 1.0,
                });
                out.push(PanelText::centered(
                    match aim {
                        Aim::Cast => "Cast: choose direction",
                        Aim::Look => "Look: choose a tile",
                    },
                    Rect::new(16.0, 243.0, 132.0, 24.0),
                ));
            }
        }
        out
    }
}

fn notice(world: &World, message: &Notice) -> String {
    let mut text = PanelText::string(world, "minigame", &message.key, "")
        .replace("\\n", " ")
        .replace("%%", "\u{0}");
    for argument in &message.arguments {
        let argument = if argument.starts_with("ow_") {
            PanelText::string(world, "minigame", argument, argument)
        } else {
            argument.clone()
        };
        if let Some(index) = text.find("%s").into_iter().chain(text.find("%d")).min() {
            text.replace_range(index..index + 2, &argument);
        }
    }
    text.replace('\u{0}', "%")
}

fn can_cast(world: &World, entity: EntityId, quest: &QuestInfo) -> bool {
    super::super::keypad::object_state(world, entity) == dark::properties::ObjectState::Hacked
        && quest
            .overworld
            .game
            .as_ref()
            .is_some_and(|g| g.player.spell_cooldown == 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{SeedableRng, rngs::StdRng};

    #[test]
    fn controller_preserves_diagonal_movement_and_missile_aim() {
        let mut world = World::new();
        let entity = world.add_entity(dark::properties::PropObjState(
            dark::properties::ObjectState::Hacked,
        ));
        let mut quest = QuestInfo::new();
        quest.overworld.game = Some(crate::overworld::OverWorld::new(
            true,
            &mut StdRng::seed_from_u64(3),
        ));
        world.add_unique(quest);
        let mut adventure = Adventure::default();
        for x in [-1, 0, 1] {
            for y in [-1, 0, 1] {
                if x == 0 && y == 0 {
                    continue;
                }
                adventure.controller(
                    entity,
                    &world,
                    HandheldControl::Stick {
                        x: x as f32 * 0.8,
                        y: -y as f32 * 0.8,
                    },
                );
                let action =
                    adventure.controller(entity, &world, HandheldControl::Navigate { x, y: 0 });
                assert!(
                    matches!(action, Effect::UpdateOverWorld { command: Command::Act(Action::Move(dx, dy)), .. } if (dx, dy) == (x, y))
                );
                adventure.input(entity, &world, Message::Cast);
                let action =
                    adventure.controller(entity, &world, HandheldControl::Navigate { x, y: 0 });
                assert!(
                    matches!(action, Effect::UpdateOverWorld { command: Command::Act(Action::Missile(dx, dy)), .. } if (dx, dy) == (x, y))
                );
            }
        }
    }
}
