//! The Transluce script family: entities that fade their render alpha in and
//! out at a fixed rate (50%/second in the original scripts). Used by the CS9
//! cutscene's holographic exhibits (eggs, grubs, rumblers), which are authored
//! with a partial `PropRenderAlpha` and composed with the exhibit scripts on
//! the same entity - a single TurnOn both stages the exhibit and fades it in.
use dark::properties::PropRenderAlpha;
use shipyard::{EntityId, Get, View, World};

use crate::{physics::PhysicsWorld, time::Time};

use super::{Effect, MessagePayload, Script};

/// Fade rate: alpha units per second ("fade in or out 50% per second").
const FADE_RATE: f32 = 0.5;

/// Drives an entity's render alpha toward a target, one `SetRenderAlpha` per
/// frame while moving. Shared by the Transluce scripts and the CS9 screens.
pub struct AlphaFader {
    current: f32,
    target: f32,
}

impl AlphaFader {
    pub fn new(initial: f32) -> AlphaFader {
        AlphaFader {
            current: initial,
            target: initial,
        }
    }

    pub fn fade_to(&mut self, target: f32) {
        self.target = target.clamp(0.0, 1.0);
    }

    /// Jump immediately (no fade) - e.g. to start invisible before a fade-in.
    pub fn snap_to(&mut self, value: f32) {
        self.current = value.clamp(0.0, 1.0);
        self.target = self.current;
    }

    pub fn update(&mut self, entity_id: EntityId, time: &Time) -> Effect {
        if self.current == self.target {
            return Effect::NoEffect;
        }
        let step = FADE_RATE * time.elapsed.as_secs_f32();
        self.current = if self.current < self.target {
            (self.current + step).min(self.target)
        } else {
            (self.current - step).max(self.target)
        };
        Effect::SetRenderAlpha {
            entity_id,
            alpha: self.current,
        }
    }
}

/// Script `TransluceInOutHolo`: a hologram that starts invisible, fades in to
/// its authored alpha on TurnOn, and fades back out on TurnOff.
pub struct TransluceInOutHolo {
    fader: AlphaFader,
    /// The authored "visible" alpha (PropRenderAlpha), captured at initialize.
    visible_alpha: f32,
}

impl TransluceInOutHolo {
    pub fn new() -> TransluceInOutHolo {
        TransluceInOutHolo {
            fader: AlphaFader::new(0.0),
            visible_alpha: 1.0,
        }
    }
}

impl Script for TransluceInOutHolo {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        let v_alpha = world.borrow::<View<PropRenderAlpha>>().unwrap();
        self.visible_alpha = v_alpha.get(entity_id).map(|a| a.0).unwrap_or(1.0);
        self.fader.snap_to(0.0);

        // Holograms are hidden until something fades them in.
        Effect::SetRenderAlpha {
            entity_id,
            alpha: 0.0,
        }
    }

    fn handle_message(
        &mut self,
        _entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { from: _ } => self.fader.fade_to(self.visible_alpha),
            MessagePayload::TurnOff { from: _ } => self.fader.fade_to(0.0),
            _ => {}
        }
        Effect::NoEffect
    }

    fn update(
        &mut self,
        entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        self.fader.update(entity_id, time)
    }
}
